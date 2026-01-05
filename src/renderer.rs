use ash::vk;
use crate::vulkan::VulkanContext;
use crate::scene::{Scene, Vertex, Material};
use crate::camera::Camera;
use winit::window::Window;
use winit::keyboard::KeyCode;
use winit::event::ElementState;
use std::mem::size_of;
use glam::{Mat4, Vec4};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraProperties {
    view_inverse: Mat4,
    proj_inverse: Mat4,
    light_pos: Vec4,
    settings: Vec4, // x: soft_shadows, y: reflections, z: refraction, w: sss
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SceneDesc {
    vertex_addr: u64,
    index_addr: u64,
    material_addr: u64,
}

#[allow(dead_code)]
pub struct Renderer {
    ctx: VulkanContext,
    command_pool: vk::CommandPool,
    command_buffers: Vec<vk::CommandBuffer>,
    
    // Resources
    vertex_buffer: (vk::Buffer, vk::DeviceMemory),
    index_buffer: (vk::Buffer, vk::DeviceMemory),
    material_buffer: (vk::Buffer, vk::DeviceMemory),
    scene_desc_buffer: (vk::Buffer, vk::DeviceMemory),
    uniform_buffer: (vk::Buffer, vk::DeviceMemory),
    
    // AS
    blas_list: Vec<(vk::AccelerationStructureKHR, vk::DeviceMemory, vk::Buffer)>,
    tlas: (vk::AccelerationStructureKHR, vk::DeviceMemory, vk::Buffer),
    
    // Pipeline
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set: vk::DescriptorSet,
    descriptor_set_layout: vk::DescriptorSetLayout,
    
    // SBT
    sbt_buffer: (vk::Buffer, vk::DeviceMemory),
    sbt_regions: [vk::StridedDeviceAddressRegionKHR; 4],
    
    // Image
    storage_image: (vk::Image, vk::DeviceMemory, vk::ImageView),
    
    // Swapchain & Sync
    swapchain: vk::SwapchainKHR,
    swapchain_images: Vec<vk::Image>,
    swapchain_image_views: Vec<vk::ImageView>,
    image_available_semaphores: Vec<vk::Semaphore>,
    render_finished_semaphores: Vec<vk::Semaphore>,
    in_flight_fences: Vec<vk::Fence>,
    
    // State
    pub camera: Camera,
    pub settings: Vec4,
    pub current_frame: usize,
    
    scene: Scene,
}

impl Renderer {
    pub fn new(window: &Window) -> Result<Self, Box<dyn std::error::Error>> {
        let ctx = VulkanContext::new(window)?;

        log::info!("Creating scene...");
        let scene = Scene::new();
        let camera = Camera::new();
        let settings = Vec4::new(1.0, 1.0, 1.0, 1.0);

        log::info!("Creating command pool...");
        let command_pool_info = vk::CommandPoolCreateInfo {
            queue_family_index: ctx.queue_family_index,
            flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            ..Default::default()
        };
        let command_pool = unsafe { ctx.device.create_command_pool(&command_pool_info, None)? };

        // Create multiple command buffers (one per frame in flight, simplified to 2)
        let max_frames = 2;
        let alloc_info = vk::CommandBufferAllocateInfo {
            command_pool,
            level: vk::CommandBufferLevel::PRIMARY,
            command_buffer_count: max_frames as u32,
            ..Default::default()
        };
        let command_buffers = unsafe { ctx.device.allocate_command_buffers(&alloc_info)? };

        log::info!("Creating scene buffers...");
        // 1. Create Buffers (Scene)
        let (vertex_buffer, vertex_mem, vertex_addr) = create_buffer_with_addr(&ctx, 
            (scene.meshes.iter().map(|m| m.vertices.len()).sum::<usize>() * size_of::<Vertex>()) as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
        )?;
        
        let (index_buffer, index_mem, index_addr) = create_buffer_with_addr(&ctx,
            (scene.meshes.iter().map(|m| m.indices.len()).sum::<usize>() * size_of::<u32>()) as u64,
             vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
             vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
        )?;

        let (material_buffer, material_mem, material_addr) = create_buffer_with_addr(&ctx,
            (scene.materials.len() * size_of::<Material>()) as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
        )?;

        upload_data(&ctx, vertex_mem, &scene.meshes.iter().flat_map(|m| m.vertices.clone()).collect::<Vec<_>>());
        upload_data(&ctx, index_mem, &scene.meshes.iter().flat_map(|m| m.indices.clone()).collect::<Vec<_>>());
        upload_data(&ctx, material_mem, &scene.materials);

        let (scene_desc_buffer, scene_desc_mem, _) = create_buffer_with_addr(&ctx,
            (scene.objects.len() * size_of::<SceneDesc>()) as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
        )?;
        
        let mut scene_descs = Vec::new();
        for obj in &scene.objects {
            // Find correct offset for this object's mesh
            let mut v_off = 0;
            let mut i_off = 0;
             for (idx, mesh) in scene.meshes.iter().enumerate() {
                 if idx == obj.mesh_index {
                     break;
                 }
                 v_off += mesh.vertices.len();
                 i_off += mesh.indices.len();
            }
            scene_descs.push(SceneDesc {
                vertex_addr: vertex_addr + (v_off * size_of::<Vertex>()) as u64,
                index_addr: index_addr + (i_off * size_of::<u32>()) as u64,
                material_addr,
            });
        }
        upload_data(&ctx, scene_desc_mem, &scene_descs);

        log::info!("Building Bottom-Level Acceleration Structures (BLAS) for {} meshes...", scene.meshes.len());
        // 2. BLAS
        let mut blas_list = Vec::new();
        let mut cur_v = 0;
        let mut cur_i = 0;
        let setup_cmd_buffer = command_buffers[0]; // Use first for setup
        
        for mesh in &scene.meshes {
            let max_vertex = mesh.vertices.len() as u32;
            let primitive_count = (mesh.indices.len() / 3) as u32;

            let triangles = vk::AccelerationStructureGeometryTrianglesDataKHR {
                vertex_format: vk::Format::R32G32B32_SFLOAT,
                vertex_data: vk::DeviceOrHostAddressConstKHR { device_address: vertex_addr + (cur_v * size_of::<Vertex>()) as u64 },
                vertex_stride: size_of::<Vertex>() as u64,
                max_vertex,
                index_type: vk::IndexType::UINT32,
                index_data: vk::DeviceOrHostAddressConstKHR { device_address: index_addr + (cur_i * size_of::<u32>()) as u64 },
                ..Default::default()
            };

            let geometry = vk::AccelerationStructureGeometryKHR {
                geometry_type: vk::GeometryTypeKHR::TRIANGLES,
                geometry: vk::AccelerationStructureGeometryDataKHR { triangles },
                flags: vk::GeometryFlagsKHR::OPAQUE,
                ..Default::default()
            };

            let geometries = [geometry];
            
            let build_info = vk::AccelerationStructureBuildGeometryInfoKHR {
                ty: vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL,
                flags: vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE,
                mode: vk::BuildAccelerationStructureModeKHR::BUILD,
                geometry_count: 1,
                p_geometries: geometries.as_ptr(),
                ..Default::default()
            };

            let mut size_info = vk::AccelerationStructureBuildSizesInfoKHR::default();
            unsafe { ctx.as_loader.get_acceleration_structure_build_sizes(vk::AccelerationStructureBuildTypeKHR::DEVICE, &build_info, &[primitive_count], &mut size_info) };

            let (as_buffer, as_mem, _) = create_buffer_with_addr(&ctx, size_info.acceleration_structure_size, vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
            
            let create_info = vk::AccelerationStructureCreateInfoKHR {
                buffer: as_buffer,
                size: size_info.acceleration_structure_size,
                ty: vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL,
                ..Default::default()
            };
            
            let accel_struct = unsafe { ctx.as_loader.create_acceleration_structure(&create_info, None)? };
            let (scratch_buf, scratch_mem, scratch_addr) = create_buffer_with_addr(&ctx, size_info.build_scratch_size, vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;

            let mut build_info = build_info;
            build_info.scratch_data = vk::DeviceOrHostAddressKHR { device_address: scratch_addr };
            build_info.dst_acceleration_structure = accel_struct;

            let build_range = vk::AccelerationStructureBuildRangeInfoKHR {
                primitive_count,
                primitive_offset: 0,
                first_vertex: 0,
                transform_offset: 0,
            };
            
            begin_single_time_command(&ctx, command_pool, setup_cmd_buffer);
            unsafe { ctx.as_loader.cmd_build_acceleration_structures(setup_cmd_buffer, &[build_info], &[&[build_range]]) };
            end_single_time_command(&ctx, command_pool, setup_cmd_buffer, ctx.queue);

            unsafe { ctx.device.destroy_buffer(scratch_buf, None); ctx.device.free_memory(scratch_mem, None); }
            blas_list.push((accel_struct, as_mem, as_buffer));
            
            cur_v += mesh.vertices.len();
            cur_i += mesh.indices.len();
        }

        log::info!("Building Top-Level Acceleration Structure (TLAS)...");
        // 3. TLAS
        let mut instances = Vec::new();
        for (_i, obj) in scene.objects.iter().enumerate() {
             let transform = obj.transform.to_cols_array_2d();
             let vk_transform = vk::TransformMatrixKHR {
                 matrix: [
                     transform[0][0], transform[1][0], transform[2][0], transform[3][0],
                     transform[0][1], transform[1][1], transform[2][1], transform[3][1],
                     transform[0][2], transform[1][2], transform[2][2], transform[3][2],
                 ]
             };
             let instance = vk::AccelerationStructureInstanceKHR {
                 transform: vk_transform,
                 instance_custom_index_and_mask: vk::Packed24_8::new(obj.material_index as u32, 0xFF), 
                 instance_shader_binding_table_record_offset_and_flags: vk::Packed24_8::new(0, vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE.as_raw() as u8),
                 acceleration_structure_reference: vk::AccelerationStructureReferenceKHR { 
                     device_handle: unsafe { ctx.as_loader.get_acceleration_structure_device_address(&vk::AccelerationStructureDeviceAddressInfoKHR { 
                         acceleration_structure: blas_list[obj.mesh_index].0,
                         ..Default::default()
                     }) }
                 },
             };
             instances.push(instance);
        }

        let (inst_buf, inst_mem, inst_addr) = create_buffer_with_addr(&ctx, (instances.len() * size_of::<vk::AccelerationStructureInstanceKHR>()) as u64, vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)?;
        upload_data(&ctx, inst_mem, &instances);

        let instances_data = vk::AccelerationStructureGeometryInstancesDataKHR {
            data: vk::DeviceOrHostAddressConstKHR { device_address: inst_addr },
            ..Default::default()
        };

        let geometry = vk::AccelerationStructureGeometryKHR {
            geometry_type: vk::GeometryTypeKHR::INSTANCES,
            geometry: vk::AccelerationStructureGeometryDataKHR { instances: instances_data },
            ..Default::default()
        };
        
        let build_info = vk::AccelerationStructureBuildGeometryInfoKHR {
            ty: vk::AccelerationStructureTypeKHR::TOP_LEVEL,
            flags: vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE,
            mode: vk::BuildAccelerationStructureModeKHR::BUILD,
            geometry_count: 1,
            p_geometries: &geometry,
            ..Default::default()
        };
        
        let primitive_count = instances.len() as u32;
        let mut size_info = vk::AccelerationStructureBuildSizesInfoKHR::default();
        unsafe { ctx.as_loader.get_acceleration_structure_build_sizes(vk::AccelerationStructureBuildTypeKHR::DEVICE, &build_info, &[primitive_count], &mut size_info) };

        let (tlas_buf, tlas_mem, _) = create_buffer_with_addr(&ctx, size_info.acceleration_structure_size, vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
        let tlas_create_info = vk::AccelerationStructureCreateInfoKHR {
            buffer: tlas_buf,
            size: size_info.acceleration_structure_size,
            ty: vk::AccelerationStructureTypeKHR::TOP_LEVEL,
            ..Default::default()
        };
        let tlas = unsafe { ctx.as_loader.create_acceleration_structure(&tlas_create_info, None)? };

        let (scratch_buf, scratch_mem, scratch_addr) = create_buffer_with_addr(&ctx, size_info.build_scratch_size, vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
        let mut build_info = build_info;
        build_info.scratch_data = vk::DeviceOrHostAddressKHR { device_address: scratch_addr };
        build_info.dst_acceleration_structure = tlas;

        let build_range = vk::AccelerationStructureBuildRangeInfoKHR {
            primitive_count,
            primitive_offset: 0,
            first_vertex: 0,
            transform_offset: 0,
        };
        
        begin_single_time_command(&ctx, command_pool, setup_cmd_buffer);
        unsafe { ctx.as_loader.cmd_build_acceleration_structures(setup_cmd_buffer, &[build_info], &[&[build_range]]) };
        end_single_time_command(&ctx, command_pool, setup_cmd_buffer, ctx.queue);
        
        unsafe { ctx.device.destroy_buffer(scratch_buf, None); ctx.device.free_memory(scratch_mem, None); ctx.device.destroy_buffer(inst_buf, None); ctx.device.free_memory(inst_mem, None); }
        let tlas_res = (tlas, tlas_mem, tlas_buf);

        log::info!("Creating storage image and swapchain...");
        // 4. Images & Swapchain
        let capabilities = unsafe { ctx.surface_loader.get_physical_device_surface_capabilities(ctx.physical_device, ctx.surface)? };
        let format = vk::Format::B8G8R8A8_UNORM;

        // Handle special case where surface extent is u32::MAX (means we should use window size)
        let extent = if capabilities.current_extent.width == u32::MAX {
            let window_size = window.inner_size();
            log::info!("Surface extent is undefined ({}), using window size: {}x{}",
                u32::MAX, window_size.width, window_size.height);
            vk::Extent2D {
                width: window_size.width.clamp(
                    capabilities.min_image_extent.width,
                    capabilities.max_image_extent.width
                ),
                height: window_size.height.clamp(
                    capabilities.min_image_extent.height,
                    capabilities.max_image_extent.height
                ),
            }
        } else {
            log::info!("Surface extent: {}x{}", capabilities.current_extent.width, capabilities.current_extent.height);
            capabilities.current_extent
        };

        // Validate extent
        if extent.width == 0 || extent.height == 0 {
            return Err(format!("Invalid extent: {}x{} - window may be minimized",
                extent.width, extent.height).into());
        }

        let storage_size_mb = (extent.width as u64 * extent.height as u64 * 4) / (1024 * 1024);
        log::info!("Creating storage image ({} MB)...", storage_size_mb);

        let (storage_image, storage_mem) = create_image(&ctx, extent.width, extent.height, format, vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC)?;
        let storage_view_info = vk::ImageViewCreateInfo {
            image: storage_image,
            view_type: vk::ImageViewType::TYPE_2D,
            format,
            subresource_range: vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            },
            ..Default::default()
        };
        let storage_view = unsafe { ctx.device.create_image_view(&storage_view_info, None)? };
        
        begin_single_time_command(&ctx, command_pool, setup_cmd_buffer);
        let barrier = vk::ImageMemoryBarrier {
            old_layout: vk::ImageLayout::UNDEFINED,
            new_layout: vk::ImageLayout::GENERAL,
            image: storage_image,
            subresource_range: vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            },
            ..Default::default()
        };
        unsafe { ctx.device.cmd_pipeline_barrier(setup_cmd_buffer, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TOP_OF_PIPE, vk::DependencyFlags::empty(), &[], &[], &[barrier]) };
        end_single_time_command(&ctx, command_pool, setup_cmd_buffer, ctx.queue);

        let swapchain_create_info = vk::SwapchainCreateInfoKHR {
            surface: ctx.surface,
            min_image_count: std::cmp::max(3, capabilities.min_image_count),
            image_format: format,
            image_color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
            image_extent: extent,
            image_array_layers: 1,
            image_usage: vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST,
            pre_transform: vk::SurfaceTransformFlagsKHR::IDENTITY,
            composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
            present_mode: vk::PresentModeKHR::FIFO,
            clipped: vk::TRUE,
            ..Default::default()
        };
        let swapchain = unsafe { ctx.swapchain_loader.create_swapchain(&swapchain_create_info, None)? };
        let swapchain_images = unsafe { ctx.swapchain_loader.get_swapchain_images(swapchain)? };
        let swapchain_image_views: Vec<vk::ImageView> = swapchain_images.iter().map(|&img| {
            unsafe { ctx.device.create_image_view(&vk::ImageViewCreateInfo {
                image: img,
                view_type: vk::ImageViewType::TYPE_2D,
                format,
                subresource_range: vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                ..Default::default()
            }, None).unwrap() }
        }).collect();

        log::info!("Creating descriptors and ray tracing pipeline...");
        // 5. Descriptors & Pipeline
        let descriptor_pool_sizes = [
            vk::DescriptorPoolSize { ty: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR, descriptor_count: 1 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_IMAGE, descriptor_count: 1 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER, descriptor_count: 1 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_BUFFER, descriptor_count: 1 },
        ];
        let descriptor_pool_info = vk::DescriptorPoolCreateInfo {
            max_sets: 1,
            pool_size_count: descriptor_pool_sizes.len() as u32,
            p_pool_sizes: descriptor_pool_sizes.as_ptr(),
            ..Default::default()
        };
        let descriptor_pool = unsafe { ctx.device.create_descriptor_pool(&descriptor_pool_info, None)? };

        let dsl_bindings = [
            vk::DescriptorSetLayoutBinding { binding: 0, descriptor_type: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR, descriptor_count: 1, stage_flags: vk::ShaderStageFlags::RAYGEN_KHR | vk::ShaderStageFlags::CLOSEST_HIT_KHR, ..Default::default() },
            vk::DescriptorSetLayoutBinding { binding: 1, descriptor_type: vk::DescriptorType::STORAGE_IMAGE, descriptor_count: 1, stage_flags: vk::ShaderStageFlags::RAYGEN_KHR, ..Default::default() },
            vk::DescriptorSetLayoutBinding { binding: 2, descriptor_type: vk::DescriptorType::UNIFORM_BUFFER, descriptor_count: 1, stage_flags: vk::ShaderStageFlags::RAYGEN_KHR | vk::ShaderStageFlags::CLOSEST_HIT_KHR, ..Default::default() },
            vk::DescriptorSetLayoutBinding { binding: 3, descriptor_type: vk::DescriptorType::STORAGE_BUFFER, descriptor_count: 1, stage_flags: vk::ShaderStageFlags::CLOSEST_HIT_KHR, ..Default::default() },
        ];
        let descriptor_set_layout_info = vk::DescriptorSetLayoutCreateInfo {
            binding_count: dsl_bindings.len() as u32,
            p_bindings: dsl_bindings.as_ptr(),
            ..Default::default()
        };
        let descriptor_set_layout = unsafe { ctx.device.create_descriptor_set_layout(&descriptor_set_layout_info, None)? };

        let alloc_info = vk::DescriptorSetAllocateInfo {
            descriptor_pool,
            descriptor_set_count: 1,
            p_set_layouts: &descriptor_set_layout,
            ..Default::default()
        };
        let descriptor_set = unsafe { ctx.device.allocate_descriptor_sets(&alloc_info)?[0] };

        let (uniform_buffer, uniform_mem, _) = create_buffer_with_addr(&ctx, size_of::<CameraProperties>() as u64, vk::BufferUsageFlags::UNIFORM_BUFFER, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)?;

        let mut tlas_write = vk::WriteDescriptorSetAccelerationStructureKHR {
            acceleration_structure_count: 1,
            p_acceleration_structures: &tlas,
            ..Default::default()
        };
        let descriptor_writes = [
            vk::WriteDescriptorSet {
                dst_set: descriptor_set,
                dst_binding: 0,
                descriptor_count: 1,
                descriptor_type: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
                p_next: &mut tlas_write as *mut _ as *mut _,
                ..Default::default()
            },
            vk::WriteDescriptorSet {
                dst_set: descriptor_set,
                dst_binding: 1,
                descriptor_count: 1,
                descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
                p_image_info: &vk::DescriptorImageInfo {
                    image_view: storage_view,
                    image_layout: vk::ImageLayout::GENERAL,
                    ..Default::default()
                },
                ..Default::default()
            },
            vk::WriteDescriptorSet {
                dst_set: descriptor_set,
                dst_binding: 2,
                descriptor_count: 1,
                descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                p_buffer_info: &vk::DescriptorBufferInfo {
                    buffer: uniform_buffer,
                    offset: 0,
                    range: vk::WHOLE_SIZE,
                },
                ..Default::default()
            },
            vk::WriteDescriptorSet {
                dst_set: descriptor_set,
                dst_binding: 3,
                descriptor_count: 1,
                descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                p_buffer_info: &vk::DescriptorBufferInfo {
                    buffer: scene_desc_buffer,
                    offset: 0,
                    range: vk::WHOLE_SIZE,
                },
                ..Default::default()
            },
        ];
        unsafe { ctx.device.update_descriptor_sets(&descriptor_writes, &[]); }

        let pipeline_layout_info = vk::PipelineLayoutCreateInfo {
            set_layout_count: 1,
            p_set_layouts: &descriptor_set_layout,
            ..Default::default()
        };
        let pipeline_layout = unsafe { ctx.device.create_pipeline_layout(&pipeline_layout_info, None)? };

        let rgen_code = compile_shader("src/shaders/raygen.rgen", shaderc::ShaderKind::RayGeneration, "main")?;
        let rmiss_code = compile_shader("src/shaders/miss.rmiss", shaderc::ShaderKind::Miss, "main")?;
        let rchit_code = compile_shader("src/shaders/closesthit.rchit", shaderc::ShaderKind::ClosestHit, "main")?;
        let shadow_miss_code = compile_shader("src/shaders/shadow.rmiss", shaderc::ShaderKind::Miss, "main")?;

        let entry_name = std::ffi::CStr::from_bytes_with_nul(b"main\0").unwrap();
        let shader_stages = [
            vk::PipelineShaderStageCreateInfo {
                stage: vk::ShaderStageFlags::RAYGEN_KHR,
                module: unsafe { ctx.device.create_shader_module(&vk::ShaderModuleCreateInfo { code_size: rgen_code.len() * 4, p_code: rgen_code.as_ptr(), ..Default::default() }, None)? },
                p_name: entry_name.as_ptr(),
                ..Default::default()
            },
            vk::PipelineShaderStageCreateInfo {
                stage: vk::ShaderStageFlags::MISS_KHR,
                module: unsafe { ctx.device.create_shader_module(&vk::ShaderModuleCreateInfo { code_size: rmiss_code.len() * 4, p_code: rmiss_code.as_ptr(), ..Default::default() }, None)? },
                p_name: entry_name.as_ptr(),
                ..Default::default()
            },
            vk::PipelineShaderStageCreateInfo {
                stage: vk::ShaderStageFlags::CLOSEST_HIT_KHR,
                module: unsafe { ctx.device.create_shader_module(&vk::ShaderModuleCreateInfo { code_size: rchit_code.len() * 4, p_code: rchit_code.as_ptr(), ..Default::default() }, None)? },
                p_name: entry_name.as_ptr(),
                ..Default::default()
            },
            vk::PipelineShaderStageCreateInfo {
                stage: vk::ShaderStageFlags::MISS_KHR,
                module: unsafe { ctx.device.create_shader_module(&vk::ShaderModuleCreateInfo { code_size: shadow_miss_code.len() * 4, p_code: shadow_miss_code.as_ptr(), ..Default::default() }, None)? },
                p_name: entry_name.as_ptr(),
                ..Default::default()
            },
        ];

        let shader_groups = [
            vk::RayTracingShaderGroupCreateInfoKHR { ty: vk::RayTracingShaderGroupTypeKHR::GENERAL, general_shader: 0, closest_hit_shader: vk::SHADER_UNUSED_KHR, any_hit_shader: vk::SHADER_UNUSED_KHR, intersection_shader: vk::SHADER_UNUSED_KHR, ..Default::default() }, 
            vk::RayTracingShaderGroupCreateInfoKHR { ty: vk::RayTracingShaderGroupTypeKHR::GENERAL, general_shader: 1, closest_hit_shader: vk::SHADER_UNUSED_KHR, any_hit_shader: vk::SHADER_UNUSED_KHR, intersection_shader: vk::SHADER_UNUSED_KHR, ..Default::default() },
            vk::RayTracingShaderGroupCreateInfoKHR { ty: vk::RayTracingShaderGroupTypeKHR::TRIANGLES_HIT_GROUP, general_shader: vk::SHADER_UNUSED_KHR, closest_hit_shader: 2, any_hit_shader: vk::SHADER_UNUSED_KHR, intersection_shader: vk::SHADER_UNUSED_KHR, ..Default::default() },
            vk::RayTracingShaderGroupCreateInfoKHR { ty: vk::RayTracingShaderGroupTypeKHR::GENERAL, general_shader: 3, closest_hit_shader: vk::SHADER_UNUSED_KHR, any_hit_shader: vk::SHADER_UNUSED_KHR, intersection_shader: vk::SHADER_UNUSED_KHR, ..Default::default() },
        ];

        let pipeline_info = vk::RayTracingPipelineCreateInfoKHR {
            stage_count: shader_stages.len() as u32,
            p_stages: shader_stages.as_ptr(),
            group_count: shader_groups.len() as u32,
            p_groups: shader_groups.as_ptr(),
            max_pipeline_ray_recursion_depth: 10,
            layout: pipeline_layout,
            ..Default::default()
        };
        let pipeline = unsafe { ctx.rt_pipeline_loader.create_ray_tracing_pipelines(vk::DeferredOperationKHR::null(), vk::PipelineCache::null(), &[pipeline_info], None).map_err(|(_, err)| err)?[0] };

        // 6. SBT (Corrected)
        let group_count = shader_groups.len() as u32;
        let prog_size = 32;
        let sbt_size = (group_count * prog_size) as u64;
        let (sbt_buffer, sbt_mem, sbt_addr) = create_buffer_with_addr(&ctx, sbt_size, vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS | vk::BufferUsageFlags::TRANSFER_SRC, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)?;
        
        let handles = unsafe { ctx.rt_pipeline_loader.get_ray_tracing_shader_group_handles(pipeline, 0, group_count, group_count as usize * 32)? };
        let mut sbt_data = vec![0u8; sbt_size as usize];
        sbt_data[0..32].copy_from_slice(&handles[0..32]); // Gen (Group 0)
        sbt_data[32..64].copy_from_slice(&handles[32..64]); // Miss 0 (Group 1)
        sbt_data[64..96].copy_from_slice(&handles[96..128]); // Miss 1 (Group 3 - Shadow)
        sbt_data[96..128].copy_from_slice(&handles[64..96]); // Hit (Group 2)
        upload_data(&ctx, sbt_mem, &sbt_data);
        
        let sbt_regions = [
            vk::StridedDeviceAddressRegionKHR { device_address: sbt_addr, stride: 32, size: 32 }, // Gen
            vk::StridedDeviceAddressRegionKHR { device_address: sbt_addr + 32, stride: 32, size: 64 }, // Miss (2 shaders)
            vk::StridedDeviceAddressRegionKHR { device_address: sbt_addr + 96, stride: 32, size: 32 }, // Hit
            vk::StridedDeviceAddressRegionKHR { device_address: 0, stride: 0, size: 0 },
        ];

        // Sync Objects
        let mut image_available_semaphores = Vec::new();
        let mut render_finished_semaphores = Vec::new();
        let mut in_flight_fences = Vec::new();
        let semaphore_info = vk::SemaphoreCreateInfo::default();
        let fence_info = vk::FenceCreateInfo {
            flags: vk::FenceCreateFlags::SIGNALED,
            ..Default::default()
        };
        
        for _ in 0..max_frames {
            image_available_semaphores.push(unsafe { ctx.device.create_semaphore(&semaphore_info, None)? });
            render_finished_semaphores.push(unsafe { ctx.device.create_semaphore(&semaphore_info, None)? });
            in_flight_fences.push(unsafe { ctx.device.create_fence(&fence_info, None)? });
        }

        Ok(Self {
            ctx,
            command_pool,
            command_buffers,
            vertex_buffer: (vertex_buffer, vertex_mem),
            index_buffer: (index_buffer, index_mem),
            material_buffer: (material_buffer, material_mem),
            scene_desc_buffer: (scene_desc_buffer, scene_desc_mem),
            uniform_buffer: (uniform_buffer, uniform_mem),
            blas_list,
            tlas: tlas_res,
            pipeline,
            pipeline_layout,
            descriptor_pool,
            descriptor_set,
            descriptor_set_layout,
            sbt_buffer: (sbt_buffer, sbt_mem),
            sbt_regions,
            storage_image: (storage_image, storage_mem, storage_view),
            swapchain,
            swapchain_images,
            swapchain_image_views,
            image_available_semaphores,
            render_finished_semaphores,
            in_flight_fences,
            camera,
            settings,
            current_frame: 0,
            scene,
        })
    }
    
    pub fn resize(&mut self, _width: u32, _height: u32) {
        // Placeholder for resize logic (requires device idle, cleanup swapchain, recreate)
    }

    pub fn handle_input(&mut self, key: KeyCode, state: ElementState) {
        if state == ElementState::Pressed {
            self.camera.handle_input(key);
            match key {
                KeyCode::Digit1 => self.settings.x = 1.0 - self.settings.x,
                KeyCode::Digit2 => self.settings.y = 1.0 - self.settings.y,
                KeyCode::Digit3 => self.settings.z = 1.0 - self.settings.z,
                KeyCode::Digit4 => self.settings.w = 1.0 - self.settings.w,
                _ => {}
            }
        }
    }
    
    pub fn handle_window_event(&mut self, _event: &winit::event::WindowEvent) {}

    pub fn render(&mut self, _window: &Window) -> Result<(), Box<dyn std::error::Error>> {
        self.camera.update_vectors();
        
        unsafe { self.ctx.device.wait_for_fences(&[self.in_flight_fences[self.current_frame]], true, u64::MAX)?; }
        
        let (image_index, _) = match unsafe { self.ctx.swapchain_loader.acquire_next_image(self.swapchain, u64::MAX, self.image_available_semaphores[self.current_frame], vk::Fence::null()) } {
            Ok(result) => result,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return Ok(()), // Should resize
            Err(e) => return Err(e.into()),
        };

        unsafe { self.ctx.device.reset_fences(&[self.in_flight_fences[self.current_frame]])?; }

        let cmd_buffer = self.command_buffers[self.current_frame];
        unsafe { self.ctx.device.reset_command_buffer(cmd_buffer, vk::CommandBufferResetFlags::empty())?; }

        // Update Uniforms
        let proj = self.camera.proj_matrix(1280.0/720.0); // Fixed aspect for now
        let view = self.camera.view_matrix();
        let ubo = CameraProperties {
            view_inverse: view.inverse(),
            proj_inverse: proj.inverse(),
            light_pos: Vec4::new(10.0, 10.0, 10.0, 1.0),
            settings: self.settings,
        };
        upload_data(&self.ctx, self.uniform_buffer.1, &vec![ubo]);

        let begin_info = vk::CommandBufferBeginInfo {
            flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
            ..Default::default()
        };
        unsafe { self.ctx.device.begin_command_buffer(cmd_buffer, &begin_info)?; }

        // Trace Rays
        unsafe {
            self.ctx.device.cmd_bind_pipeline(cmd_buffer, vk::PipelineBindPoint::RAY_TRACING_KHR, self.pipeline);
            self.ctx.device.cmd_bind_descriptor_sets(cmd_buffer, vk::PipelineBindPoint::RAY_TRACING_KHR, self.pipeline_layout, 0, &[self.descriptor_set], &[]);
            self.ctx.rt_pipeline_loader.cmd_trace_rays(
                cmd_buffer,
                &self.sbt_regions[0],
                &self.sbt_regions[1],
                &self.sbt_regions[2],
                &self.sbt_regions[3],
                1280, 720, 1
            );
        }

        // Blit to Swapchain
        let subresource = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };
        
        // Transition Storage to Transfer Src
        let barrier1 = vk::ImageMemoryBarrier {
            old_layout: vk::ImageLayout::GENERAL,
            new_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            image: self.storage_image.0,
            subresource_range: subresource,
            src_access_mask: vk::AccessFlags::SHADER_WRITE,
            dst_access_mask: vk::AccessFlags::TRANSFER_READ,
            ..Default::default()
        };
        
        // Transition Swapchain to Transfer Dst
        let barrier2_fix = vk::ImageMemoryBarrier {
            old_layout: vk::ImageLayout::UNDEFINED,
            new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            image: self.swapchain_images[image_index as usize],
            subresource_range: subresource,
            src_access_mask: vk::AccessFlags::empty(),
            dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
            ..Default::default()
        };

        unsafe {
            self.ctx.device.cmd_pipeline_barrier(cmd_buffer, vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[barrier1, barrier2_fix]);
            
            let blit = vk::ImageBlit {
                src_offsets: [vk::Offset3D { x: 0, y: 0, z: 0 }, vk::Offset3D { x: 1280, y: 720, z: 1 }],
                src_subresource: vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 },
                dst_offsets: [vk::Offset3D { x: 0, y: 0, z: 0 }, vk::Offset3D { x: 1280, y: 720, z: 1 }],
                dst_subresource: vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 },
            };
            
            self.ctx.device.cmd_blit_image(cmd_buffer, self.storage_image.0, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, self.swapchain_images[image_index as usize], vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[blit], vk::Filter::NEAREST);
            
            // Transition Swapchain to Present
             let barrier3 = vk::ImageMemoryBarrier {
                old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                new_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                image: self.swapchain_images[image_index as usize],
                subresource_range: subresource,
                src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                dst_access_mask: vk::AccessFlags::empty(),
                ..Default::default()
            };
            
            // Transition Storage back to General
             let barrier4 = vk::ImageMemoryBarrier {
                old_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                new_layout: vk::ImageLayout::GENERAL,
                image: self.storage_image.0,
                subresource_range: subresource,
                src_access_mask: vk::AccessFlags::TRANSFER_READ,
                dst_access_mask: vk::AccessFlags::empty(),
                ..Default::default()
            };

             self.ctx.device.cmd_pipeline_barrier(cmd_buffer, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::BOTTOM_OF_PIPE, vk::DependencyFlags::empty(), &[], &[], &[barrier3, barrier4]);
        
             self.ctx.device.end_command_buffer(cmd_buffer)?;
        }

        let submit_info = vk::SubmitInfo {
            wait_semaphore_count: 1,
            p_wait_semaphores: &self.image_available_semaphores[self.current_frame],
            p_wait_dst_stage_mask: &vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            command_buffer_count: 1,
            p_command_buffers: &cmd_buffer,
            signal_semaphore_count: 1,
            p_signal_semaphores: &self.render_finished_semaphores[self.current_frame],
            ..Default::default()
        };

        unsafe { self.ctx.device.queue_submit(self.ctx.queue, &[submit_info], self.in_flight_fences[self.current_frame])?; }

        let present_info = vk::PresentInfoKHR {
            wait_semaphore_count: 1,
            p_wait_semaphores: &self.render_finished_semaphores[self.current_frame],
            swapchain_count: 1,
            p_swapchains: &self.swapchain,
            p_image_indices: &image_index,
            ..Default::default()
        };

        match unsafe { self.ctx.swapchain_loader.queue_present(self.ctx.queue, &present_info) } {
             Ok(_) => {},
             Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {},
             Err(e) => return Err(e.into()),
        }

        self.current_frame = (self.current_frame + 1) % 2;

        Ok(())
    }
}

// Helpers (Same as before)
fn create_buffer_with_addr(ctx: &VulkanContext, size: u64, usage: vk::BufferUsageFlags, props: vk::MemoryPropertyFlags) -> Result<(vk::Buffer, vk::DeviceMemory, u64), Box<dyn std::error::Error>> {
    let create_info = vk::BufferCreateInfo {
        size,
        usage,
        sharing_mode: vk::SharingMode::EXCLUSIVE,
        ..Default::default()
    };

    let buffer = unsafe { ctx.device.create_buffer(&create_info, None)? };
    let mem_req = unsafe { ctx.device.get_buffer_memory_requirements(buffer) };
    let mem_type_index = find_memory_type(ctx, mem_req.memory_type_bits, props)?;

    log::debug!("Allocating buffer: {} bytes (required: {} bytes, alignment: {})",
        size, mem_req.size, mem_req.alignment);

    let mut flags = vk::MemoryAllocateFlagsInfo {
        flags: vk::MemoryAllocateFlags::DEVICE_ADDRESS,
        ..Default::default()
    };
    let alloc_info = vk::MemoryAllocateInfo {
        allocation_size: mem_req.size,
        memory_type_index: mem_type_index,
        p_next: &mut flags as *mut _ as *mut _,
        ..Default::default()
    };

    let memory = match unsafe { ctx.device.allocate_memory(&alloc_info, None) } {
        Ok(m) => m,
        Err(e) => {
            log::error!("Failed to allocate {} bytes of GPU memory (usage: {:?}, props: {:?})",
                mem_req.size, usage, props);
            return Err(format!("Memory allocation failed: {} - requested {} MB",
                e, mem_req.size / (1024 * 1024)).into());
        }
    };

    unsafe { ctx.device.bind_buffer_memory(buffer, memory, 0)? };

    let addr_info = vk::BufferDeviceAddressInfo {
        buffer,
        ..Default::default()
    };
    let addr = unsafe { ctx.device.get_buffer_device_address(&addr_info) };

    Ok((buffer, memory, addr))
}

fn create_image(ctx: &VulkanContext, width: u32, height: u32, format: vk::Format, usage: vk::ImageUsageFlags) -> Result<(vk::Image, vk::DeviceMemory), Box<dyn std::error::Error>> {
    let create_info = vk::ImageCreateInfo {
        image_type: vk::ImageType::TYPE_2D,
        format,
        extent: vk::Extent3D { width, height, depth: 1 },
        mip_levels: 1,
        array_layers: 1,
        samples: vk::SampleCountFlags::TYPE_1,
        tiling: vk::ImageTiling::OPTIMAL,
        usage,
        sharing_mode: vk::SharingMode::EXCLUSIVE,
        initial_layout: vk::ImageLayout::UNDEFINED,
        ..Default::default()
    };

    let image = unsafe { ctx.device.create_image(&create_info, None)? };
    let mem_req = unsafe { ctx.device.get_image_memory_requirements(image) };

    log::debug!("Image memory requirements: {} MB (alignment: {})",
        mem_req.size / (1024 * 1024), mem_req.alignment);

    let mem_type_index = find_memory_type(ctx, mem_req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
    let alloc_info = vk::MemoryAllocateInfo {
        allocation_size: mem_req.size,
        memory_type_index: mem_type_index,
        ..Default::default()
    };

    let memory = match unsafe { ctx.device.allocate_memory(&alloc_info, None) } {
        Ok(m) => m,
        Err(e) => {
            log::error!("Failed to allocate image memory: {} MB for {}x{} image",
                mem_req.size / (1024 * 1024), width, height);
            return Err(format!("Image allocation failed: {} - requested {} MB",
                e, mem_req.size / (1024 * 1024)).into());
        }
    };

    unsafe { ctx.device.bind_image_memory(image, memory, 0)? };

    Ok((image, memory))
}


fn find_memory_type(ctx: &VulkanContext, type_filter: u32, properties: vk::MemoryPropertyFlags) -> Result<u32, Box<dyn std::error::Error>> {
    let mem_properties = unsafe { ctx.instance.get_physical_device_memory_properties(ctx.physical_device) };
    for i in 0..mem_properties.memory_type_count {
        if (type_filter & (1 << i)) != 0 && (mem_properties.memory_types[i as usize].property_flags & properties) == properties {
            return Ok(i);
        }
    }
    Err("Failed to find suitable memory type".into())
}

fn upload_data<T: Copy>(ctx: &VulkanContext, memory: vk::DeviceMemory, data: &[T]) {
    let size = (data.len() * size_of::<T>()) as u64;
    let ptr = unsafe { ctx.device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()).unwrap() };
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, ptr as *mut u8, size as usize) };
    unsafe { ctx.device.unmap_memory(memory) };
}

fn begin_single_time_command(ctx: &VulkanContext, _pool: vk::CommandPool, buffer: vk::CommandBuffer) {
    let begin_info = vk::CommandBufferBeginInfo {
        flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
        ..Default::default()
    };
    unsafe { ctx.device.begin_command_buffer(buffer, &begin_info).unwrap() };
}

fn end_single_time_command(ctx: &VulkanContext, _pool: vk::CommandPool, buffer: vk::CommandBuffer, queue: vk::Queue) {
    unsafe { ctx.device.end_command_buffer(buffer).unwrap() };
    let submit_info = vk::SubmitInfo {
        command_buffer_count: 1,
        p_command_buffers: &buffer,
        ..Default::default()
    };
    unsafe { ctx.device.queue_submit(queue, &[submit_info], vk::Fence::null()).unwrap() };
    unsafe { ctx.device.queue_wait_idle(queue).unwrap() };
}

fn compile_shader(path: &str, kind: shaderc::ShaderKind, entry: &str) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(path)?;
    let compiler = shaderc::Compiler::new().unwrap();
    let mut options = shaderc::CompileOptions::new().unwrap();
    options.set_target_env(shaderc::TargetEnv::Vulkan, shaderc::EnvVersion::Vulkan1_2 as u32);
    options.set_target_spirv(shaderc::SpirvVersion::V1_4);
    
    let binary = compiler.compile_into_spirv(&source, kind, path, entry, Some(&options))?;
    Ok(binary.as_binary().to_vec())
}