# Ash - Vulkan Bindings for Rust

Lightweight Vulkan bindings with type-safe wrappers around raw Vulkan API.

## Key Features
- Near 1:1 mapping to Vulkan C API
- Type-safe builders for Vulkan structures (`.default()` pattern)
- Extension loading system
- `linked` feature for static linking to Vulkan loader

## Common Patterns

### Instance Creation
```rust
use ash::{vk, Entry};
let entry = Entry::linked();
let app_info = vk::ApplicationInfo::default()
    .application_name(c"MyApp")
    .api_version(vk::make_api_version(0, 1, 3, 0));
let create_info = vk::InstanceCreateInfo::default()
    .application_info(&app_info);
let instance = entry.create_instance(&create_info, None)?;
```

### Device Creation Pattern
1. Pick physical device: `instance.enumerate_physical_devices()`
2. Find queue family: `instance.get_physical_device_queue_family_properties()`
3. Create logical device with `vk::DeviceCreateInfo`
4. Get queue handle: `device.get_device_queue()`

### Memory Allocation
```rust
// Find suitable memory type
let mem_reqs = device.get_buffer_memory_requirements(buffer);
let mem_type_index = find_memory_type(
    mem_reqs.memory_type_bits,
    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
);

// Allocate
let alloc_info = vk::MemoryAllocateInfo::default()
    .allocation_size(mem_reqs.size)
    .memory_type_index(mem_type_index);
let memory = device.allocate_memory(&alloc_info, None)?;
device.bind_buffer_memory(buffer, memory, 0)?;
```

### Command Buffer Pattern
```rust
// Allocate
let alloc_info = vk::CommandBufferAllocateInfo::default()
    .command_pool(pool)
    .level(vk::CommandBufferLevel::PRIMARY)
    .command_buffer_count(1);
let cmd_bufs = device.allocate_command_buffers(&alloc_info)?;

// Record
device.begin_command_buffer(cmd_bufs[0], &begin_info)?;
// ... record commands ...
device.end_command_buffer(cmd_bufs[0])?;

// Submit
let submit_info = vk::SubmitInfo::default()
    .command_buffers(&cmd_bufs);
device.queue_submit(queue, &[submit_info], fence)?;
```

### Builder Pattern
All Vulkan structures use `.default()` + builder methods:
```rust
let info = vk::BufferCreateInfo::default()
    .size(1024)
    .usage(vk::BufferUsageFlags::VERTEX_BUFFER)
    .sharing_mode(vk::SharingMode::EXCLUSIVE);
```

## Resource Management
- **Critical**: All Vulkan resources must be manually destroyed in reverse creation order
- Use RAII wrappers or explicit cleanup functions
- Always call `device.device_wait_idle()` before destroying resources
- Pattern: Store resources in structs with `destroy()` methods

## Extensions
```rust
use ash::khr;
let swapchain_loader = khr::swapchain::Device::new(&instance, &device);
let surface_loader = khr::surface::Instance::new(&entry, &instance);
```

## Ray Tracing Extensions
- `ash::khr::acceleration_structure::Device` - BLAS/TLAS creation
- `ash::khr::ray_tracing_pipeline::Device` - RT pipeline, SBT
- Required extensions: `VK_KHR_acceleration_structure`, `VK_KHR_ray_tracing_pipeline`, `VK_KHR_deferred_host_operations`

## Safety Notes
- Most ash functions are `unsafe` - caller must ensure Vulkan validity rules
- Use builders to avoid uninitialized fields
- Match Vulkan object lifetimes (no dangling handles)
- Synchronization is manual (fences, semaphores, barriers)
