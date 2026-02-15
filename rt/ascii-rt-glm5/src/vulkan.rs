//! Vulkan setup for GPU-accelerated ray tracing
//!
//! This module provides Vulkan initialization with ray tracing extensions.

use ash::vk;
use ash::Entry;
use ash::Instance;
use std::ffi::CString;

/// Vulkan context containing instance and device
pub struct VulkanContext {
    pub entry: Entry,
    pub instance: Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: ash::Device,
    pub graphics_queue: vk::Queue,
    pub graphics_queue_family_index: u32,
    pub command_pool: vk::CommandPool,
}

/// Required device extensions for ray tracing
const DEVICE_EXTENSIONS: &[&str] = &[
    "VK_KHR_ray_tracing_pipeline",
    "VK_KHR_acceleration_structure",
    "VK_KHR_buffer_device_address",
    "VK_KHR_deferred_host_operations",
    "VK_EXT_descriptor_indexing",
    "VK_KHR_spirv_1_4",
    "VK_KHR_shader_float_controls",
];

impl VulkanContext {
    /// Create a new Vulkan context with ray tracing support
    pub fn new() -> Result<Self, String> {
        unsafe {
            let entry = Entry::load()
                .map_err(|e| format!("Failed to load Vulkan entry: {}", e))?;

            let instance = Self::create_instance(&entry)?;
            let physical_device = Self::pick_physical_device(&instance)?;
            let (device, graphics_queue_family_index) =
                Self::create_device(&instance, physical_device)?;

            let graphics_queue = device.get_device_queue(graphics_queue_family_index, 0);
            let command_pool = Self::create_command_pool(&device, graphics_queue_family_index)?;

            Ok(Self {
                entry,
                instance,
                physical_device,
                device,
                graphics_queue,
                graphics_queue_family_index,
                command_pool,
            })
        }
    }

    unsafe fn create_instance(entry: &Entry) -> Result<Instance, String> {
        let app_name = CString::new("ASCII Raytracer").unwrap();
        let engine_name = CString::new("No Engine").unwrap();

        let app_info = vk::ApplicationInfo::default()
            .application_name(&app_name)
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_name(&engine_name)
            .engine_version(vk::make_api_version(0, 0, 0, 0))
            .api_version(vk::API_VERSION_1_2);

        let extension_names = Self::get_required_instance_extensions();
        let extension_ptrs: Vec<*const i8> = extension_names.iter().map(|e| e.as_ptr()).collect();

        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&extension_ptrs);

        let instance = entry
            .create_instance(&create_info, None)
            .map_err(|e| format!("Failed to create Vulkan instance: {}", e))?;

        Ok(instance)
    }

    fn get_required_instance_extensions() -> Vec<CString> {
        vec![
            CString::new("VK_KHR_get_physical_device_properties2").unwrap(),
            CString::new("VK_EXT_debug_utils").unwrap(),
        ]
    }

    unsafe fn pick_physical_device(instance: &Instance) -> Result<vk::PhysicalDevice, String> {
        let devices = instance
            .enumerate_physical_devices()
            .map_err(|e| format!("Failed to enumerate physical devices: {}", e))?;

        if devices.is_empty() {
            return Err("No physical devices found".to_string());
        }

        // Just pick the first suitable device
        for &device in &devices {
            if Self::is_device_suitable(instance, device) {
                return Ok(device);
            }
        }

        // Fallback to first device if none have ray tracing
        Ok(devices[0])
    }

    unsafe fn is_device_suitable(instance: &Instance, device: vk::PhysicalDevice) -> bool {
        let features = instance.get_physical_device_features(device);

        // Check for ray tracing support
        let device_extensions: Vec<CString> = DEVICE_EXTENSIONS
            .iter()
            .map(|s| CString::new(*s).unwrap())
            .collect();

        let extension_props = instance
            .enumerate_device_extension_properties(device)
            .unwrap_or_default();

        let has_all_extensions = device_extensions.iter().all(|ext| {
            extension_props.iter().any(|prop| {
                let prop_name = std::ffi::CStr::from_ptr(prop.extension_name.as_ptr());
                prop_name == ext.as_c_str()
            })
        });

        features.geometry_shader != 0 && has_all_extensions
    }

    unsafe fn create_device(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
    ) -> Result<(ash::Device, u32), String> {
        let queue_family_index = Self::find_graphics_queue_family(instance, physical_device)?;

        let queue_priorities = [1.0f32];
        let queue_create_infos = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&queue_priorities)];

        let device_extensions: Vec<CString> = DEVICE_EXTENSIONS
            .iter()
            .map(|s| CString::new(*s).unwrap())
            .collect();
        let extension_ptrs: Vec<*const i8> = device_extensions.iter().map(|e| e.as_ptr()).collect();

        let physical_features = vk::PhysicalDeviceFeatures::default()
            .geometry_shader(true)
            .shader_int64(true);

        // Vulkan 1.2 features
        let mut vulkan_12_features = vk::PhysicalDeviceVulkan12Features::default()
            .buffer_device_address(true)
            .shader_float16(true)
            .shader_int8(true);

        let create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_create_infos)
            .enabled_extension_names(&extension_ptrs)
            .enabled_features(&physical_features)
            .push_next(&mut vulkan_12_features);

        let device = instance
            .create_device(physical_device, &create_info, None)
            .map_err(|e| format!("Failed to create Vulkan device: {}", e))?;

        Ok((device, queue_family_index))
    }

    unsafe fn find_graphics_queue_family(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
    ) -> Result<u32, String> {
        let queue_families = instance.get_physical_device_queue_family_properties(physical_device);

        for (index, family) in queue_families.iter().enumerate() {
            if family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                return Ok(index as u32);
            }
        }

        Err("No graphics queue family found".to_string())
    }

    unsafe fn create_command_pool(
        device: &ash::Device,
        queue_family_index: u32,
    ) -> Result<vk::CommandPool, String> {
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);

        device
            .create_command_pool(&pool_info, None)
            .map_err(|e| format!("Failed to create command pool: {}", e))
    }

    /// Check if ray tracing is available on this device
    pub fn is_ray_tracing_available(&self) -> bool {
        unsafe {
            let extension_props = self
                .instance
                .enumerate_device_extension_properties(self.physical_device)
                .unwrap_or_default();

            let rt_extensions = [
                "VK_KHR_ray_tracing_pipeline",
                "VK_KHR_acceleration_structure",
            ];

            rt_extensions.iter().all(|ext| {
                let ext_name = CString::new(*ext).unwrap();
                extension_props.iter().any(|prop| {
                    let prop_name = std::ffi::CStr::from_ptr(prop.extension_name.as_ptr());
                    prop_name == ext_name.as_c_str()
                })
            })
        }
    }

    /// Get device name
    pub fn get_device_name(&self) -> String {
        unsafe {
            let props = self.instance.get_physical_device_properties(self.physical_device);
            let name = std::ffi::CStr::from_ptr(props.device_name.as_ptr());
            name.to_string_lossy().to_string()
        }
    }
}

impl Drop for VulkanContext {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

/// Create a simple test to verify Vulkan is working
pub fn test_vulkan() -> Result<String, String> {
    match VulkanContext::new() {
        Ok(ctx) => {
            let device_name = ctx.get_device_name();
            let rt_available = ctx.is_ray_tracing_available();
            let status = if rt_available {
                format!("Vulkan initialized successfully on '{}'. Ray tracing available!", device_name)
            } else {
                format!("Vulkan initialized on '{}'. Ray tracing not available (using CPU fallback).", device_name)
            };
            Ok(status)
        }
        Err(e) => Err(format!("Vulkan initialization failed: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vulkan_context_creation() {
        // This test may fail in CI environments without Vulkan
        if let Ok(ctx) = VulkanContext::new() {
            assert!(!ctx.get_device_name().is_empty());
        }
    }

    #[test]
    fn test_ray_tracing_check() {
        if let Ok(ctx) = VulkanContext::new() {
            // Just check it doesn't panic
            let _ = ctx.is_ray_tracing_available();
        }
    }
}
