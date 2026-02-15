use ash::vk;
use std::ffi::CStr;

#[allow(dead_code)]
pub struct VulkanDevice {
    pub device: ash::Device,
    pub physical_device: vk::PhysicalDevice,
    pub queue: vk::Queue,
    pub queue_family_index: u32,
    pub memory_properties: vk::PhysicalDeviceMemoryProperties,
    pub shader_group_handle_size: u32,
    pub shader_group_base_alignment: u32,
    pub accel_structure: ash::khr::acceleration_structure::Device,
    pub rt_pipeline: ash::khr::ray_tracing_pipeline::Device,
    pub swapchain_loader: ash::khr::swapchain::Device,
}

impl VulkanDevice {
    pub fn new(vk_instance: &super::VulkanInstance) -> Result<Self, Box<dyn std::error::Error>> {
        let instance = &vk_instance.instance;

        let required_device_extensions: [&CStr; 6] = [
            ash::khr::ray_tracing_pipeline::NAME,
            ash::khr::acceleration_structure::NAME,
            ash::khr::deferred_host_operations::NAME,
            ash::khr::buffer_device_address::NAME,
            ash::khr::ray_tracing_position_fetch::NAME,
            ash::khr::swapchain::NAME,
        ];

        // Pick physical device with RT + presentation support
        let (physical_device, queue_family_index) = unsafe {
            let devices = instance.enumerate_physical_devices()?;
            let mut found = None;
            for pd in devices {
                let exts = instance.enumerate_device_extension_properties(pd)?;
                let ext_names: Vec<&CStr> = exts
                    .iter()
                    .map(|e| CStr::from_ptr(e.extension_name.as_ptr()))
                    .collect();
                let has_all = required_device_extensions
                    .iter()
                    .all(|req| ext_names.contains(req));
                if !has_all {
                    continue;
                }

                let qf_props = instance.get_physical_device_queue_family_properties(pd);
                let qf = qf_props.iter().enumerate().find(|(idx, props)| {
                    if props.queue_count == 0 || !props.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                        return false;
                    }
                    // Check presentation support
                    let present_support = vk_instance
                        .surface_loader
                        .get_physical_device_surface_support(pd, *idx as u32, vk_instance.surface)
                        .unwrap_or(false);
                    present_support
                });
                if let Some((idx, _)) = qf {
                    let props = instance.get_physical_device_properties(pd);
                    let name = CStr::from_ptr(props.device_name.as_ptr());
                    log::info!("Selected GPU: {:?}", name);
                    found = Some((pd, idx as u32));
                    break;
                }
            }
            found.ok_or("No suitable GPU with ray tracing + presentation support found")?
        };

        let priorities = [1.0f32];
        let queue_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&priorities);

        let mut features12 = vk::PhysicalDeviceVulkan12Features::default()
            .buffer_device_address(true)
            .vulkan_memory_model(true);

        let mut as_features = vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default()
            .acceleration_structure(true);

        let mut rt_features = vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default()
            .ray_tracing_pipeline(true);

        let mut pos_fetch_features =
            vk::PhysicalDeviceRayTracingPositionFetchFeaturesKHR::default()
                .ray_tracing_position_fetch(true);

        let ext_name_ptrs: Vec<*const i8> =
            required_device_extensions.iter().map(|e| e.as_ptr()).collect();

        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(std::slice::from_ref(&queue_info))
            .enabled_extension_names(&ext_name_ptrs)
            .push_next(&mut features12)
            .push_next(&mut as_features)
            .push_next(&mut rt_features)
            .push_next(&mut pos_fetch_features);

        let device = unsafe { instance.create_device(physical_device, &device_info, None)? };
        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };
        let memory_properties =
            unsafe { instance.get_physical_device_memory_properties(physical_device) };

        let mut rt_pipeline_properties =
            vk::PhysicalDeviceRayTracingPipelinePropertiesKHR::default();
        let mut props2 =
            vk::PhysicalDeviceProperties2::default().push_next(&mut rt_pipeline_properties);
        unsafe { instance.get_physical_device_properties2(physical_device, &mut props2) };

        log::info!(
            "RT properties: handle_size={}, base_alignment={}",
            rt_pipeline_properties.shader_group_handle_size,
            rt_pipeline_properties.shader_group_base_alignment
        );

        let accel_structure =
            ash::khr::acceleration_structure::Device::new(instance, &device);
        let rt_pipeline = ash::khr::ray_tracing_pipeline::Device::new(instance, &device);
        let swapchain_loader = ash::khr::swapchain::Device::new(instance, &device);

        Ok(VulkanDevice {
            device,
            physical_device,
            queue,
            queue_family_index,
            memory_properties,
            shader_group_handle_size: rt_pipeline_properties.shader_group_handle_size,
            shader_group_base_alignment: rt_pipeline_properties.shader_group_base_alignment,
            accel_structure,
            rt_pipeline,
            swapchain_loader,
        })
    }

    pub fn create_command_pool(&self) -> Result<vk::CommandPool, vk::Result> {
        let info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(self.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        unsafe { self.device.create_command_pool(&info, None) }
    }
}

impl Drop for VulkanDevice {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().ok();
            self.device.destroy_device(None);
        }
    }
}
