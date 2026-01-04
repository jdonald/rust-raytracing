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

        // Physical Device
        let pdevices = unsafe { instance.enumerate_physical_devices()? };
        let (physical_device, queue_family_index) = pdevices
            .iter()
            .find_map(|pdevice| {
                unsafe {
                    let props = instance.get_physical_device_properties(*pdevice);
                    let queue_families = instance.get_physical_device_queue_family_properties(*pdevice);
                    
                    let q_index = queue_families.iter().enumerate().find(|(_, q)| {
                        q.queue_flags.contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE) &&
                        surface_loader.get_physical_device_surface_support(*pdevice, 0, surface).unwrap_or(false)
                    }).map(|(i, _)| i as u32);

                    if props.device_type == vk::PhysicalDeviceType::DISCRETE_GPU && q_index.is_some() {
                        // Check extensions support (simplified)
                        Some((*pdevice, q_index.unwrap()))
                    } else {
                        None
                    }
                }
            })
            .ok_or("No suitable GPU found")?;

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
