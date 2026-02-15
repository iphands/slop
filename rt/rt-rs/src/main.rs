mod render;
mod scene;
mod vk;

use ash::vk as avk;
use render::{CameraUBO, StorageImage, HEIGHT, WIDTH};
use scene::{SceneState, SceneUBO};
use vk::accel::AccelStructures;
use vk::buffer::BufferResource;
use vk::descriptor::Descriptors;
use vk::pipeline::RtPipeline;
use vk::swapchain::Swapchain;
use vk::VulkanDevice;
use vk::VulkanInstance;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

const FRAMES_IN_FLIGHT: usize = 2;

struct App {
    window: Option<Window>,
    renderer: Option<Renderer>,
}

struct Renderer {
    #[allow(dead_code)]
    vk_instance: VulkanInstance,
    vk_dev: VulkanDevice,
    swapchain: Swapchain,
    command_pool: avk::CommandPool,
    command_buffers: Vec<avk::CommandBuffer>,
    accel: AccelStructures,
    storage_image: StorageImage,
    camera_buffer: BufferResource,
    scene_buffer: BufferResource,
    descriptors: Descriptors,
    rt_pipeline: RtPipeline,
    scene_state: SceneState,
    // Sync (per swapchain image)
    image_available: Vec<avk::Semaphore>,
    render_finished: Vec<avk::Semaphore>,
    in_flight_fences: [avk::Fence; FRAMES_IN_FLIGHT],
    frame_idx: usize,
    last_time: std::time::Instant,
    frame_count: u32,
    frame_time_sum: f32,
}

impl Renderer {
    fn new(window: &Window) -> Self {
        let display_handle = window.display_handle().unwrap().as_raw();
        let window_handle = window.window_handle().unwrap().as_raw();

        let vk_instance = VulkanInstance::new(display_handle, window_handle)
            .expect("Failed to create Vulkan instance");
        let vk_dev = VulkanDevice::new(&vk_instance).expect("Failed to create device");
        let command_pool = vk_dev.create_command_pool().expect("Failed to create command pool");

        let size = window.inner_size();
        let swapchain = Swapchain::new(&vk_instance, &vk_dev, size.width, size.height);

        // Allocate command buffers
        let alloc_info = avk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(avk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(FRAMES_IN_FLIGHT as u32);
        let command_buffers = unsafe {
            vk_dev
                .device
                .allocate_command_buffers(&alloc_info)
                .unwrap()
        };

        // Build acceleration structures
        let accel = AccelStructures::build(&vk_dev, command_pool)
            .expect("Failed to build acceleration structures");

        // Storage image
        let storage_image = StorageImage::new(&vk_dev, command_pool);

        // Camera UBO
        let camera = render::create_camera_ubo();
        let camera_buffer = BufferResource::new(
            &vk_dev.device,
            vk_dev.memory_properties,
            std::mem::size_of::<CameraUBO>() as u64,
            avk::BufferUsageFlags::UNIFORM_BUFFER,
            avk::MemoryPropertyFlags::HOST_VISIBLE | avk::MemoryPropertyFlags::HOST_COHERENT,
        );
        camera_buffer.store(bytemuck::bytes_of(&camera), &vk_dev.device);

        // Scene UBO
        let scene_state = SceneState::new();
        let scene_ubo = scene_state.to_ubo();
        let scene_buffer = BufferResource::new(
            &vk_dev.device,
            vk_dev.memory_properties,
            std::mem::size_of::<SceneUBO>() as u64,
            avk::BufferUsageFlags::UNIFORM_BUFFER,
            avk::MemoryPropertyFlags::HOST_VISIBLE | avk::MemoryPropertyFlags::HOST_COHERENT,
        );
        scene_buffer.store(bytemuck::bytes_of(&scene_ubo), &vk_dev.device);

        // Descriptors
        let descriptors = Descriptors::new(&vk_dev.device).expect("Failed to create descriptors");
        descriptors.update(
            &vk_dev.device,
            accel.tlas,
            storage_image.view,
            camera_buffer.buffer,
            std::mem::size_of::<CameraUBO>() as u64,
            scene_buffer.buffer,
            std::mem::size_of::<SceneUBO>() as u64,
        );

        // Pipeline
        let rt_pipeline =
            RtPipeline::new(&vk_dev, descriptors.set_layout).expect("Failed to create pipeline");

        // Sync primitives (one semaphore pair per swapchain image)
        let sem_info = avk::SemaphoreCreateInfo::default();
        let fence_info =
            avk::FenceCreateInfo::default().flags(avk::FenceCreateFlags::SIGNALED);
        let image_count = swapchain.images.len();
        let image_available = unsafe {
            (0..image_count)
                .map(|_| vk_dev.device.create_semaphore(&sem_info, None).unwrap())
                .collect()
        };
        let render_finished = unsafe {
            (0..image_count)
                .map(|_| vk_dev.device.create_semaphore(&sem_info, None).unwrap())
                .collect()
        };
        let in_flight_fences = unsafe {
            [
                vk_dev.device.create_fence(&fence_info, None).unwrap(),
                vk_dev.device.create_fence(&fence_info, None).unwrap(),
            ]
        };

        Renderer {
            vk_instance,
            vk_dev,
            swapchain,
            command_pool,
            command_buffers,
            accel,
            storage_image,
            camera_buffer,
            scene_buffer,
            descriptors,
            rt_pipeline,
            scene_state,
            image_available,
            render_finished,
            in_flight_fences,
            frame_idx: 0,
            last_time: std::time::Instant::now(),
            frame_count: 0,
            frame_time_sum: 0.0,
        }
    }

    fn render_frame(&mut self) {
        let device = &self.vk_dev.device;
        let fi = self.frame_idx;

        // 1. Wait fence (ensures previous frame using this fence has completed)
        unsafe {
            device
                .wait_for_fences(&[self.in_flight_fences[fi]], true, u64::MAX)
                .unwrap();
        }

        // 2. Acquire swapchain image - acquireNextImage returns WHICH image, then signals image_available[that_index]
        let (image_index_u32, _suboptimal) = unsafe {
            match self
                .vk_dev
                .swapchain_loader
                .acquire_next_image(
                    self.swapchain.swapchain,
                    u64::MAX,
                    avk::Semaphore::null(),
                    avk::Fence::null(),
                ) {
                Ok(result) => result,
                Err(avk::Result::ERROR_OUT_OF_DATE_KHR) => return,
                Err(e) => panic!("Failed to acquire swapchain image: {:?}", e),
            }
        };
        let image_index = image_index_u32 as usize;

        // Reset fence after acquire so it can be signaled again
        unsafe {
            device
                .reset_fences(&[self.in_flight_fences[fi]])
                .unwrap();
        }

        // 3. Update scene
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last_time).as_secs_f32();
        let frame_time_ms = dt * 1000.0;
        self.last_time = now;

        // Track frame timing
        self.frame_count += 1;
        self.frame_time_sum += frame_time_ms;
        if self.frame_count >= 30 {
            let avg_frame_time = self.frame_time_sum / self.frame_count as f32;
            let fps = 1000.0 / avg_frame_time;
            println!("[Frame {:<6}] {:.2} ms/frame ({:.1} FPS)", self.frame_count, avg_frame_time, fps);
            self.frame_count = 0;
            self.frame_time_sum = 0.0;
        }

        self.scene_state.update(dt);

        // Update sphere instance transform
        let sphere_center = self.scene_state.sphere_center();
        self.accel
            .update_sphere_transform(&self.vk_dev.device, sphere_center);

        // 4. Update scene UBO
        let scene_ubo = self.scene_state.to_ubo();
        self.scene_buffer
            .store(bytemuck::bytes_of(&scene_ubo), &self.vk_dev.device);

        // 5. Record command buffer
        let cmd = self.command_buffers[fi];
        unsafe {
            device
                .reset_command_buffer(cmd, avk::CommandBufferResetFlags::empty())
                .unwrap();
            device
                .begin_command_buffer(
                    cmd,
                    &avk::CommandBufferBeginInfo::default()
                        .flags(avk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .unwrap();
        }

        render::record_frame_commands(
            &self.vk_dev,
            cmd,
            &self.accel,
            &self.rt_pipeline,
            &self.descriptors,
            self.storage_image.image,
            self.swapchain.images[image_index as usize],
            self.swapchain.extent,
        );

        unsafe {
            device.end_command_buffer(cmd).unwrap();
        }

        // 6. Submit (no wait, signal render_finished for this image)
        let signal_semaphores = [self.render_finished[image_index]];
        let submit_info = avk::SubmitInfo::default()
            .command_buffers(std::slice::from_ref(&cmd))
            .signal_semaphores(&signal_semaphores);

        unsafe {
            device
                .queue_submit(
                    self.vk_dev.queue,
                    &[submit_info],
                    self.in_flight_fences[fi],
                )
                .unwrap();
        }

        // 7. Present
        let swapchains = [self.swapchain.swapchain];
        let image_indices = [image_index_u32];
        let present_info = avk::PresentInfoKHR::default()
            .wait_semaphores(&signal_semaphores)
            .swapchains(&swapchains)
            .image_indices(&image_indices);

        unsafe {
            match self
                .vk_dev
                .swapchain_loader
                .queue_present(self.vk_dev.queue, &present_info)
            {
                Ok(_) | Err(avk::Result::ERROR_OUT_OF_DATE_KHR) => {}
                Err(e) => panic!("Failed to present: {:?}", e),
            }
        }

        self.frame_idx = (self.frame_idx + 1) % FRAMES_IN_FLIGHT;
    }

    fn cleanup(&mut self) {
        unsafe {
            self.vk_dev.device.device_wait_idle().unwrap();

            for sem in &self.image_available {
                self.vk_dev.device.destroy_semaphore(*sem, None);
            }
            for sem in &self.render_finished {
                self.vk_dev.device.destroy_semaphore(*sem, None);
            }
            for i in 0..FRAMES_IN_FLIGHT {
                self.vk_dev
                    .device
                    .destroy_fence(self.in_flight_fences[i], None);
            }
        }

        self.rt_pipeline.destroy(&self.vk_dev.device);
        self.descriptors.destroy(&self.vk_dev.device);
        self.scene_buffer.destroy(&self.vk_dev.device);
        self.camera_buffer.destroy(&self.vk_dev.device);
        self.storage_image.destroy(&self.vk_dev.device);
        self.accel.destroy(&self.vk_dev);
        self.swapchain.destroy(&self.vk_dev);

        unsafe {
            self.vk_dev
                .device
                .destroy_command_pool(self.command_pool, None);
        }
    }
}

impl App {
    fn new() -> Self {
        App {
            window: None,
            renderer: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title("rt-rs: Cornell Box + Glass Sphere")
            .with_inner_size(winit::dpi::LogicalSize::new(WIDTH, HEIGHT));

        let window = event_loop.create_window(attrs).unwrap();
        let renderer = Renderer::new(&window);
        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.cleanup();
                }
                self.renderer = None;
                event_loop.exit();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key_code),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                if key_code == winit::keyboard::KeyCode::Escape {
                    if let Some(renderer) = self.renderer.as_mut() {
                        renderer.cleanup();
                    }
                    self.renderer = None;
                    event_loop.exit();
                    return;
                }
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.scene_state.handle_key(key_code);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.render_frame();
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    println!("=== rt-rs: Real-Time Cornell Box + Glass Sphere ===");
    println!("Resolution: {}x{}", WIDTH, HEIGHT);
    println!("Controls:");
    println!("  Arrow keys: Move light XZ");
    println!("  PgUp/PgDn: Move light Y");
    println!("  R/G/B: Toggle light color channels");
    println!("  +/-: Adjust light intensity");
    println!("  [/]: Decrease/increase max bounces (0-31)");
    println!("  Escape: Quit");

    let event_loop = EventLoop::new()?;
    let mut app = App::new();
    event_loop.run_app(&mut app)?;

    Ok(())
}
