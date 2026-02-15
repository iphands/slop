use ash::vk;
use glam::Vec3;

use crate::vk::buffer::find_memory_type;
use crate::vk::device::VulkanDevice;

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;
pub const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUBO {
    pub origin: [f32; 3],
    pub fov: f32,
    pub forward: [f32; 3],
    pub _pad0: f32,
    pub right: [f32; 3],
    pub _pad1: f32,
    pub up: [f32; 3],
    pub aspect: f32,
}

pub struct StorageImage {
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub view: vk::ImageView,
}

impl StorageImage {
    pub fn new(vk_dev: &VulkanDevice, command_pool: vk::CommandPool) -> Self {
        let device = &vk_dev.device;

        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(COLOR_FORMAT)
            .extent(vk::Extent3D {
                width: WIDTH,
                height: HEIGHT,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(
                vk::ImageUsageFlags::STORAGE
                    | vk::ImageUsageFlags::TRANSFER_SRC,
            );

        let image = unsafe { device.create_image(&image_info, None).unwrap() };

        let mem_reqs = unsafe { device.get_image_memory_requirements(image) };
        let mem_info = vk::MemoryAllocateInfo::default()
            .allocation_size(mem_reqs.size)
            .memory_type_index(find_memory_type(
                vk_dev.memory_properties,
                mem_reqs.memory_type_bits,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            ));
        let memory = unsafe { device.allocate_memory(&mem_info, None).unwrap() };
        unsafe { device.bind_image_memory(image, memory, 0).unwrap() };

        let view_info = vk::ImageViewCreateInfo::default()
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(COLOR_FORMAT)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            })
            .image(image);
        let view = unsafe { device.create_image_view(&view_info, None).unwrap() };

        // Transition to GENERAL layout
        transition_image_layout(device, vk_dev.queue, command_pool, image);

        StorageImage {
            image,
            memory,
            view,
        }
    }

    pub fn destroy(&self, device: &ash::Device) {
        unsafe {
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

fn transition_image_layout(
    device: &ash::Device,
    queue: vk::Queue,
    pool: vk::CommandPool,
    image: vk::Image,
) {
    unsafe {
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let cmd = device.allocate_command_buffers(&alloc_info).unwrap()[0];

        device
            .begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .unwrap();

        let barrier = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::GENERAL)
            .image(image)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            });

        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );

        device.end_command_buffer(cmd).unwrap();

        let submit = vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cmd));
        device
            .queue_submit(queue, &[submit], vk::Fence::null())
            .unwrap();
        device.queue_wait_idle(queue).unwrap();
        device.free_command_buffers(pool, &[cmd]);
    }
}

pub fn create_camera_ubo() -> CameraUBO {
    let origin = Vec3::new(0.0, 0.0, 3.5);
    let target = Vec3::new(0.0, 0.0, 0.0);
    let world_up = Vec3::new(0.0, 1.0, 0.0);

    let forward = (target - origin).normalize();
    let right = forward.cross(world_up).normalize();
    let up = right.cross(forward).normalize();

    let fov = 40.0_f32.to_radians();
    let aspect = WIDTH as f32 / HEIGHT as f32;

    CameraUBO {
        origin: origin.into(),
        fov,
        forward: forward.into(),
        _pad0: 0.0,
        right: right.into(),
        _pad1: 0.0,
        up: up.into(),
        aspect,
    }
}

/// Record frame commands: TLAS rebuild, trace rays, blit storage image to swapchain image
pub fn record_frame_commands(
    vk_dev: &VulkanDevice,
    cmd: vk::CommandBuffer,
    accel: &crate::vk::accel::AccelStructures,
    rt_pipeline: &crate::vk::pipeline::RtPipeline,
    descriptors: &crate::vk::descriptor::Descriptors,
    storage_image: vk::Image,
    swapchain_image: vk::Image,
    swapchain_extent: vk::Extent2D,
) {
    let device = &vk_dev.device;

    unsafe {
        // a. TLAS rebuild
        accel.cmd_rebuild_tlas(cmd, vk_dev);

        // b. Memory barrier: AS write -> RT shader read
        let as_barrier = vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::ACCELERATION_STRUCTURE_WRITE_KHR)
            .dst_access_mask(vk::AccessFlags::ACCELERATION_STRUCTURE_READ_KHR);
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::ACCELERATION_STRUCTURE_BUILD_KHR,
            vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR,
            vk::DependencyFlags::empty(),
            &[as_barrier],
            &[],
            &[],
        );

        // c. Bind pipeline + descriptors
        device.cmd_bind_pipeline(
            cmd,
            vk::PipelineBindPoint::RAY_TRACING_KHR,
            rt_pipeline.pipeline,
        );
        device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::RAY_TRACING_KHR,
            rt_pipeline.pipeline_layout,
            0,
            &[descriptors.set],
            &[],
        );

        // d. traceRaysKHR
        vk_dev.rt_pipeline.cmd_trace_rays(
            cmd,
            &rt_pipeline.raygen_region,
            &rt_pipeline.miss_region,
            &rt_pipeline.hit_region,
            &rt_pipeline.call_region,
            WIDTH,
            HEIGHT,
            1,
        );

        let color_range = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };

        // e. Barrier: storage image GENERAL -> TRANSFER_SRC
        let storage_to_src = vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::SHADER_WRITE)
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .image(storage_image)
            .subresource_range(color_range);

        // f. Barrier: swapchain image UNDEFINED -> TRANSFER_DST
        let swap_to_dst = vk::ImageMemoryBarrier::default()
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .image(swapchain_image)
            .subresource_range(color_range);

        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[storage_to_src, swap_to_dst],
        );

        // g. Blit storage -> swapchain
        let blit_region = vk::ImageBlit {
            src_subresource: vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            },
            src_offsets: [
                vk::Offset3D { x: 0, y: 0, z: 0 },
                vk::Offset3D {
                    x: WIDTH as i32,
                    y: HEIGHT as i32,
                    z: 1,
                },
            ],
            dst_subresource: vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            },
            dst_offsets: [
                vk::Offset3D { x: 0, y: 0, z: 0 },
                vk::Offset3D {
                    x: swapchain_extent.width as i32,
                    y: swapchain_extent.height as i32,
                    z: 1,
                },
            ],
        };

        device.cmd_blit_image(
            cmd,
            storage_image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            swapchain_image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &[blit_region],
            vk::Filter::LINEAR,
        );

        // h. Barrier: swapchain -> PRESENT_SRC_KHR
        let swap_to_present = vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
            .image(swapchain_image)
            .subresource_range(color_range);

        // i. Barrier: storage image -> GENERAL
        let storage_back = vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_READ)
            .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .image(storage_image)
            .subresource_range(color_range);

        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[swap_to_present, storage_back],
        );
    }
}
