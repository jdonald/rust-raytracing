use ash::{vk, Entry, Instance, Device};
use ash::khr::{surface, swapchain, acceleration_structure, ray_tracing_pipeline};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::ffi::CString;

pub struct VulkanContext {
    pub entry: Entry,
    pub instance: Instance,
    pub surface_loader: surface::Instance,
    pub surface: vk::SurfaceKHR,
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,
    pub queue_family_index: u32,
    pub queue: vk::Queue,
    
    // Extensions
    pub swapchain_loader: swapchain::Device,
    pub as_loader: acceleration_structure::Device,
    pub rt_pipeline_loader: ray_tracing_pipeline::Device,
}

impl VulkanContext {
    pub fn new(window: &winit::window::Window) -> Result<Self, Box<dyn std::error::Error>> {
        let entry = unsafe { Entry::load()? };
        
        // Instance
        let app_name = CString::new("Rust Raytracing").unwrap();
        let engine_name = CString::new("No Engine").unwrap();
        let app_info = vk::ApplicationInfo {
            p_application_name: app_name.as_ptr(),
            application_version: 0,
            p_engine_name: engine_name.as_ptr(),
            engine_version: 0,
            api_version: vk::API_VERSION_1_2,
            ..Default::default()
        };

        let display_handle = window.display_handle()?.as_raw();
        let window_handle = window.window_handle()?.as_raw();

        let mut extension_names = ash_window::enumerate_required_extensions(display_handle)?.to_vec();
        extension_names.push(vk::EXT_DEBUG_UTILS_NAME.as_ptr());

        let create_info = vk::InstanceCreateInfo {
            p_application_info: &app_info,
            enabled_extension_count: extension_names.len() as u32,
            pp_enabled_extension_names: extension_names.as_ptr(),
            ..Default::default()
        };

        let instance = unsafe { entry.create_instance(&create_info, None)? };

        // Surface
        let surface_loader = surface::Instance::new(&entry, &instance);
        let surface = unsafe { ash_window::create_surface(&entry, &instance, display_handle, window_handle, None)? };

        // Physical Device Selection with detailed logging
        let pdevices = unsafe { instance.enumerate_physical_devices()? };

        log::info!("Found {} physical device(s)", pdevices.len());

        // Log all available devices
        for (idx, pdevice) in pdevices.iter().enumerate() {
            unsafe {
                let props = instance.get_physical_device_properties(*pdevice);
                let mem_props = instance.get_physical_device_memory_properties(*pdevice);
                let device_name = std::ffi::CStr::from_ptr(props.device_name.as_ptr())
                    .to_string_lossy();

                let device_type = match props.device_type {
                    vk::PhysicalDeviceType::DISCRETE_GPU => "Discrete GPU",
                    vk::PhysicalDeviceType::INTEGRATED_GPU => "Integrated GPU",
                    vk::PhysicalDeviceType::VIRTUAL_GPU => "Virtual GPU",
                    vk::PhysicalDeviceType::CPU => "CPU",
                    _ => "Other",
                };

                // Calculate total VRAM
                let mut total_vram: u64 = 0;
                for i in 0..mem_props.memory_heap_count {
                    let heap = mem_props.memory_heaps[i as usize];
                    if heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL) {
                        total_vram += heap.size;
                    }
                }

                log::info!("  Device {}: {} ({}) - VRAM: {} MB",
                    idx, device_name, device_type, total_vram / (1024 * 1024));

                // Check raytracing support
                let available_exts = instance.enumerate_device_extension_properties(*pdevice)
                    .unwrap_or_default();
                let has_rt = available_exts.iter().any(|ext| {
                    let name = std::ffi::CStr::from_ptr(ext.extension_name.as_ptr());
                    name == vk::KHR_RAY_TRACING_PIPELINE_NAME
                });
                let has_as = available_exts.iter().any(|ext| {
                    let name = std::ffi::CStr::from_ptr(ext.extension_name.as_ptr());
                    name == vk::KHR_ACCELERATION_STRUCTURE_NAME
                });

                log::info!("    Ray Tracing: {}, Acceleration Structure: {}", has_rt, has_as);
            }
        }

        // Score and select best device
        let mut scored_devices: Vec<(vk::PhysicalDevice, u32, u32)> = Vec::new();

        for pdevice in pdevices.iter() {
            unsafe {
                let props = instance.get_physical_device_properties(*pdevice);
                let queue_families = instance.get_physical_device_queue_family_properties(*pdevice);

                // Find suitable queue family
                let q_index = queue_families.iter().enumerate().find_map(|(i, q)| {
                    let supports_graphics = q.queue_flags.contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE);
                    let supports_present = surface_loader
                        .get_physical_device_surface_support(*pdevice, i as u32, surface)
                        .unwrap_or(false);

                    if supports_graphics && supports_present {
                        Some(i as u32)
                    } else {
                        None
                    }
                });

                if let Some(queue_idx) = q_index {
                    // Check required extensions
                    let available_exts = instance.enumerate_device_extension_properties(*pdevice)
                        .unwrap_or_default();

                    let required_exts = [
                        vk::KHR_SWAPCHAIN_NAME,
                        vk::KHR_ACCELERATION_STRUCTURE_NAME,
                        vk::KHR_RAY_TRACING_PIPELINE_NAME,
                        vk::KHR_DEFERRED_HOST_OPERATIONS_NAME,
                        vk::KHR_BUFFER_DEVICE_ADDRESS_NAME,
                    ];

                    let has_all_exts = required_exts.iter().all(|required| {
                        available_exts.iter().any(|ext| {
                            let name = std::ffi::CStr::from_ptr(ext.extension_name.as_ptr());
                            name == *required
                        })
                    });

                    if has_all_exts {
                        // Score: discrete GPU = 1000, integrated = 500, other = 100
                        let mut score = match props.device_type {
                            vk::PhysicalDeviceType::DISCRETE_GPU => 1000,
                            vk::PhysicalDeviceType::INTEGRATED_GPU => 500,
                            _ => 100,
                        };

                        // Prefer devices with more VRAM
                        let mem_props = instance.get_physical_device_memory_properties(*pdevice);
                        for i in 0..mem_props.memory_heap_count {
                            let heap = mem_props.memory_heaps[i as usize];
                            if heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL) {
                                score += (heap.size / (1024 * 1024 * 1024)) as u32; // +1 per GB
                            }
                        }

                        scored_devices.push((*pdevice, queue_idx, score));
                    }
                }
            }
        }

        if scored_devices.is_empty() {
            return Err("No suitable GPU found with required Vulkan ray tracing extensions. \
                       Required: VK_KHR_ray_tracing_pipeline, VK_KHR_acceleration_structure. \
                       Please ensure your GPU supports hardware ray tracing and drivers are up to date.".into());
        }

        // Sort by score (highest first)
        scored_devices.sort_by(|a, b| b.2.cmp(&a.2));

        let (physical_device, queue_family_index) = (scored_devices[0].0, scored_devices[0].1);

        unsafe {
            let props = instance.get_physical_device_properties(physical_device);
            let device_name = std::ffi::CStr::from_ptr(props.device_name.as_ptr())
                .to_string_lossy();
            log::info!("Selected GPU: {} (score: {})", device_name, scored_devices[0].2);
        }

        // Device
        let queue_priorities = [1.0];
        let queue_info = vk::DeviceQueueCreateInfo {
            queue_family_index,
            queue_count: 1,
            p_queue_priorities: queue_priorities.as_ptr(),
            ..Default::default()
        };

        let device_extension_names = [
            vk::KHR_SWAPCHAIN_NAME.as_ptr(),
            vk::KHR_ACCELERATION_STRUCTURE_NAME.as_ptr(),
            vk::KHR_RAY_TRACING_PIPELINE_NAME.as_ptr(),
            vk::KHR_DEFERRED_HOST_OPERATIONS_NAME.as_ptr(),
            vk::KHR_SPIRV_1_4_NAME.as_ptr(),
            vk::KHR_SHADER_FLOAT_CONTROLS_NAME.as_ptr(),
            vk::KHR_BUFFER_DEVICE_ADDRESS_NAME.as_ptr(),
        ];

        let mut features12 = vk::PhysicalDeviceVulkan12Features {
            buffer_device_address: vk::TRUE,
            ..Default::default()
        };
        
        let mut as_features = vk::PhysicalDeviceAccelerationStructureFeaturesKHR {
            acceleration_structure: vk::TRUE,
            ..Default::default()
        };
            
        let mut rt_features = vk::PhysicalDeviceRayTracingPipelineFeaturesKHR {
            ray_tracing_pipeline: vk::TRUE,
            ..Default::default()
        };

        // Chain features
        as_features.p_next = &mut rt_features as *mut _ as *mut _;
        features12.p_next = &mut as_features as *mut _ as *mut _;

        let device_create_info = vk::DeviceCreateInfo {
            queue_create_info_count: 1,
            p_queue_create_infos: &queue_info,
            enabled_extension_count: device_extension_names.len() as u32,
            pp_enabled_extension_names: device_extension_names.as_ptr(),
            p_next: &mut features12 as *mut _ as *mut _,
            ..Default::default()
        };

        let device = unsafe { instance.create_device(physical_device, &device_create_info, None)? };
        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };

        let swapchain_loader = swapchain::Device::new(&instance, &device);
        let as_loader = acceleration_structure::Device::new(&instance, &device);
        let rt_pipeline_loader = ray_tracing_pipeline::Device::new(&instance, &device);

        Ok(Self {
            entry,
            instance,
            surface_loader,
            surface,
            physical_device,
            device,
            queue_family_index,
            queue,
            swapchain_loader,
            as_loader,
            rt_pipeline_loader,
        })
    }
}

impl Drop for VulkanContext {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);
            self.instance.destroy_instance(None);
        }
    }
}
