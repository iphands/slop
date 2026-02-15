# Design Patterns for Graphics Programming

## RAII Resource Management

### Vulkan Resource Wrapper
```rust
struct BufferResource {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    size: u64,
}

impl BufferResource {
    fn new(device: &ash::Device, size: u64, usage: vk::BufferUsageFlags) -> Self {
        // Create buffer, allocate memory, bind...
    }

    fn destroy(&mut self, device: &ash::Device) {
        unsafe {
            device.destroy_buffer(self.buffer, None);
            device.free_memory(self.memory, None);
        }
    }

    fn store(&self, data: &[u8], device: &ash::Device) {
        unsafe {
            let ptr = device.map_memory(self.memory, 0, self.size, vk::MemoryMapFlags::empty()).unwrap();
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
            device.unmap_memory(self.memory);
        }
    }
}
```

## Builder Pattern for Complex Initialization

### Vulkan Pipeline Builder
```rust
struct PipelineBuilder {
    shaders: Vec<vk::PipelineShaderStageCreateInfo>,
    vertex_input: Option<vk::PipelineVertexInputStateCreateInfo>,
    // ... other pipeline states
}

impl PipelineBuilder {
    fn new() -> Self { /* ... */ }

    fn add_shader(mut self, stage: vk::ShaderStageFlags, module: vk::ShaderModule) -> Self {
        self.shaders.push(/* ... */);
        self
    }

    fn set_vertex_input(mut self, bindings: &[vk::VertexInputBindingDescription]) -> Self {
        self.vertex_input = Some(/* ... */);
        self
    }

    fn build(self, device: &ash::Device, layout: vk::PipelineLayout) -> Result<vk::Pipeline> {
        // Validate and create pipeline
    }
}
```

## Double/Triple Buffering

### Frame-in-Flight Pattern
```rust
const FRAMES_IN_FLIGHT: usize = 2;

struct Renderer {
    command_buffers: Vec<vk::CommandBuffer>,
    fences: [vk::Fence; FRAMES_IN_FLIGHT],
    semaphores: [FrameSemaphores; FRAMES_IN_FLIGHT],
    frame_index: usize,
}

struct FrameSemaphores {
    image_available: vk::Semaphore,
    render_finished: vk::Semaphore,
}

impl Renderer {
    fn render_frame(&mut self) {
        let fi = self.frame_index;

        // Wait for this frame slot to be available
        unsafe { device.wait_for_fences(&[self.fences[fi]], true, u64::MAX).unwrap(); }

        // Acquire swapchain image
        let (image_index, _) = swapchain_loader.acquire_next_image(
            swapchain,
            u64::MAX,
            self.semaphores[fi].image_available,
            vk::Fence::null()
        ).unwrap();

        // Reset fence
        unsafe { device.reset_fences(&[self.fences[fi]]).unwrap(); }

        // Record and submit commands...

        self.frame_index = (self.frame_index + 1) % FRAMES_IN_FLIGHT;
    }
}
```

## Type-State Pattern for Safety

### Prevent Invalid States at Compile Time
```rust
struct Uninitialized;
struct Initialized;

struct Renderer<State> {
    state: PhantomData<State>,
    // ... other fields
}

impl Renderer<Uninitialized> {
    fn new() -> Self { /* ... */ }

    fn initialize(self) -> Result<Renderer<Initialized>> {
        // Initialization logic
        Ok(Renderer { state: PhantomData })
    }
}

impl Renderer<Initialized> {
    fn render(&mut self) {
        // Only available after initialization
    }
}

// Usage:
let renderer = Renderer::new().initialize()?;
renderer.render(); // OK
// Renderer::new().render(); // Compile error!
```

## Command Pattern for Recording

### Deferred Command Execution
```rust
trait RenderCommand {
    fn execute(&self, cmd: vk::CommandBuffer, device: &VulkanDevice);
}

struct DrawCommand {
    pipeline: vk::Pipeline,
    vertex_buffer: vk::Buffer,
    index_count: u32,
}

impl RenderCommand for DrawCommand {
    fn execute(&self, cmd: vk::CommandBuffer, device: &VulkanDevice) {
        unsafe {
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            device.cmd_bind_vertex_buffers(cmd, 0, &[self.vertex_buffer], &[0]);
            device.cmd_draw_indexed(cmd, self.index_count, 1, 0, 0, 0);
        }
    }
}

// Build command list, execute later
let commands: Vec<Box<dyn RenderCommand>> = vec![
    Box::new(DrawCommand { /* ... */ }),
    Box::new(DrawCommand { /* ... */ }),
];

for cmd in &commands {
    cmd.execute(command_buffer, &device);
}
```

## Object Pool for Temporary Resources

### Command Buffer Pool
```rust
struct CommandBufferPool {
    pool: vk::CommandPool,
    buffers: Vec<vk::CommandBuffer>,
    available: Vec<vk::CommandBuffer>,
}

impl CommandBufferPool {
    fn acquire(&mut self, device: &ash::Device) -> vk::CommandBuffer {
        self.available.pop().unwrap_or_else(|| {
            // Allocate new buffer
            let alloc_info = vk::CommandBufferAllocateInfo::default()
                .command_pool(self.pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1);
            let bufs = unsafe { device.allocate_command_buffers(&alloc_info).unwrap() };
            self.buffers.push(bufs[0]);
            bufs[0]
        })
    }

    fn release(&mut self, buffer: vk::CommandBuffer) {
        self.available.push(buffer);
    }

    fn reset_all(&mut self, device: &ash::Device) {
        unsafe {
            device.reset_command_pool(self.pool, vk::CommandPoolResetFlags::empty()).unwrap();
        }
        self.available.extend(self.buffers.iter());
    }
}
```

## Visitor Pattern for Scene Traversal

### Scene Graph Traversal
```rust
trait SceneVisitor {
    fn visit_mesh(&mut self, mesh: &Mesh);
    fn visit_light(&mut self, light: &Light);
    fn visit_camera(&mut self, camera: &Camera);
}

struct RenderVisitor {
    command_buffer: vk::CommandBuffer,
    // ... rendering state
}

impl SceneVisitor for RenderVisitor {
    fn visit_mesh(&mut self, mesh: &Mesh) {
        // Record draw commands
    }
    // ...
}

struct SceneNode {
    children: Vec<SceneNode>,
    entity: Entity,
}

impl SceneNode {
    fn accept(&self, visitor: &mut dyn SceneVisitor) {
        match &self.entity {
            Entity::Mesh(m) => visitor.visit_mesh(m),
            Entity::Light(l) => visitor.visit_light(l),
            // ...
        }
        for child in &self.children {
            child.accept(visitor);
        }
    }
}
```

## Flyweight Pattern for Shared Resources

### Material System
```rust
struct MaterialCache {
    materials: HashMap<MaterialId, Arc<Material>>,
}

impl MaterialCache {
    fn get_or_create(&mut self, id: MaterialId, create_fn: impl FnOnce() -> Material) -> Arc<Material> {
        self.materials.entry(id)
            .or_insert_with(|| Arc::new(create_fn()))
            .clone()
    }
}

// Share materials across meshes
let mat = material_cache.get_or_create(mat_id, || Material::new(...));
```
