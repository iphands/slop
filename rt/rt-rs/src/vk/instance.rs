use ash::vk;
use std::ffi::{c_void, CStr, CString};

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    msg_type: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user: *mut c_void,
) -> vk::Bool32 {
    let sev = match severity {
        vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE => "VERBOSE",
        vk::DebugUtilsMessageSeverityFlagsEXT::INFO => "INFO",
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => "WARNING",
        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => "ERROR",
        _ => "UNKNOWN",
    };
    let ty = match msg_type {
        vk::DebugUtilsMessageTypeFlagsEXT::GENERAL => "GEN",
        vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION => "VAL",
        vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE => "PERF",
        _ => "?",
    };
    let msg = unsafe { CStr::from_ptr((*data).p_message) };
    eprintln!("[VK {sev}][{ty}] {msg:?}");
    vk::FALSE
}

#[allow(dead_code)]
pub struct VulkanInstance {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub surface_loader: ash::khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
    _debug_utils: ash::ext::debug_utils::Instance,
    _debug_messenger: vk::DebugUtilsMessengerEXT,
}

impl VulkanInstance {
    pub fn new(
        display_handle: raw_window_handle::RawDisplayHandle,
        window_handle: raw_window_handle::RawWindowHandle,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let entry = ash::Entry::linked();

        let app_name = CString::new("rt-rs")?;
        let engine_name = CString::new("No Engine")?;
        let app_info = vk::ApplicationInfo::default()
            .application_name(&app_name)
            .application_version(vk::make_api_version(0, 1, 0, 0))
            .engine_name(&engine_name)
            .engine_version(vk::make_api_version(0, 1, 0, 0))
            .api_version(vk::API_VERSION_1_2);

        let layer_name = CString::new("VK_LAYER_KHRONOS_validation")?;
        let layer_names = [layer_name.as_ptr()];

        // Surface extensions required by the windowing system
        let surface_extensions =
            ash_window::enumerate_required_extensions(display_handle)?;

        let mut ext_names: Vec<*const i8> = surface_extensions.to_vec();
        ext_names.push(ash::ext::debug_utils::NAME.as_ptr());

        let mut debug_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(
                vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                    | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            )
            .pfn_user_callback(Some(debug_callback));

        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_layer_names(&layer_names)
            .enabled_extension_names(&ext_names)
            .push_next(&mut debug_info);

        let instance = unsafe { entry.create_instance(&create_info, None)? };

        let debug_utils = ash::ext::debug_utils::Instance::new(&entry, &instance);
        let debug_messenger =
            unsafe { debug_utils.create_debug_utils_messenger(&debug_info, None)? };

        // Create surface
        let surface = unsafe {
            ash_window::create_surface(&entry, &instance, display_handle, window_handle, None)?
        };
        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);

        log::info!("Vulkan instance created with validation layers and surface");

        Ok(VulkanInstance {
            entry,
            instance,
            surface_loader,
            surface,
            _debug_utils: debug_utils,
            _debug_messenger: debug_messenger,
        })
    }
}

impl Drop for VulkanInstance {
    fn drop(&mut self) {
        unsafe {
            self.surface_loader
                .destroy_surface(self.surface, None);
            self._debug_utils
                .destroy_debug_utils_messenger(self._debug_messenger, None);
            self.instance.destroy_instance(None);
        }
    }
}
