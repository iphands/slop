use ash::vk;
use super::device::VulkanDevice;
use super::instance::VulkanInstance;

#[allow(dead_code)]
pub struct Swapchain {
    pub swapchain: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub image_views: Vec<vk::ImageView>,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
}

impl Swapchain {
    pub fn new(
        vk_instance: &VulkanInstance,
        vk_dev: &VulkanDevice,
        width: u32,
        height: u32,
    ) -> Self {
        let surface_caps = unsafe {
            vk_instance
                .surface_loader
                .get_physical_device_surface_capabilities(
                    vk_dev.physical_device,
                    vk_instance.surface,
                )
                .unwrap()
        };

        let surface_formats = unsafe {
            vk_instance
                .surface_loader
                .get_physical_device_surface_formats(vk_dev.physical_device, vk_instance.surface)
                .unwrap()
        };

        // Prefer B8G8R8A8_SRGB or B8G8R8A8_UNORM, fallback to first available
        let surface_format = surface_formats
            .iter()
            .find(|f| {
                f.format == vk::Format::B8G8R8A8_SRGB
                    && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
            })
            .or_else(|| {
                surface_formats.iter().find(|f| {
                    f.format == vk::Format::B8G8R8A8_UNORM
                        && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
                })
            })
            .unwrap_or(&surface_formats[0]);

        let extent = if surface_caps.current_extent.width != u32::MAX {
            surface_caps.current_extent
        } else {
            vk::Extent2D {
                width: width.clamp(
                    surface_caps.min_image_extent.width,
                    surface_caps.max_image_extent.width,
                ),
                height: height.clamp(
                    surface_caps.min_image_extent.height,
                    surface_caps.max_image_extent.height,
                ),
            }
        };

        let mut image_count = surface_caps.min_image_count + 1;
        if surface_caps.max_image_count > 0 && image_count > surface_caps.max_image_count {
            image_count = surface_caps.max_image_count;
        }

        let create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(vk_instance.surface)
            .min_image_count(image_count)
            .image_format(surface_format.format)
            .image_color_space(surface_format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(surface_caps.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(vk::PresentModeKHR::FIFO)
            .clipped(true);

        let swapchain = unsafe {
            vk_dev
                .swapchain_loader
                .create_swapchain(&create_info, None)
                .unwrap()
        };

        let images = unsafe {
            vk_dev
                .swapchain_loader
                .get_swapchain_images(swapchain)
                .unwrap()
        };

        let image_views: Vec<vk::ImageView> = images
            .iter()
            .map(|&image| {
                let view_info = vk::ImageViewCreateInfo::default()
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(surface_format.format)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
                    .image(image);
                unsafe { vk_dev.device.create_image_view(&view_info, None).unwrap() }
            })
            .collect();

        log::info!(
            "Swapchain created: {}x{}, format {:?}, {} images",
            extent.width,
            extent.height,
            surface_format.format,
            images.len()
        );

        Swapchain {
            swapchain,
            images,
            image_views,
            format: surface_format.format,
            extent,
        }
    }

    pub fn destroy(&self, vk_dev: &VulkanDevice) {
        unsafe {
            for &view in &self.image_views {
                vk_dev.device.destroy_image_view(view, None);
            }
            vk_dev
                .swapchain_loader
                .destroy_swapchain(self.swapchain, None);
        }
    }
}
