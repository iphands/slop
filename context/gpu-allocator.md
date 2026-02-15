# gpu-allocator - GPU Memory Management

Memory allocator for Vulkan, DirectX 12, and Metal, based on AMD's Vulkan Memory Allocator (VMA).

## Key Features
- Reduces memory fragmentation
- Handles complex allocation strategies
- Supports different memory types (device-local, host-visible, etc.)
- Automatic memory type selection
- Block allocation for efficiency

## Basic Setup

### Vulkan Allocator
```rust
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};
use gpu_allocator::MemoryLocation;

let allocator = Allocator::new(&AllocatorCreateDesc {
    instance: instance.clone(),
    device: device.clone(),
    physical_device,
    debug_settings: Default::default(),
    buffer_device_address: false,
    allocation_sizes: Default::default(),
})?;
```

## Memory Locations

### MemoryLocation Enum
```rust
use gpu_allocator::MemoryLocation;

// GPU-only memory (fastest, for render targets, etc.)
MemoryLocation::GpuOnly

// CPU → GPU upload (staging buffers)
MemoryLocation::CpuToGpu

// GPU → CPU download (readback)
MemoryLocation::GpuToCpu

// Unknown/custom requirements
MemoryLocation::Unknown
```

## Allocation Pattern

### Buffer Allocation
```rust
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme};

// Create buffer
let buffer_info = vk::BufferCreateInfo::default()
    .size(size)
    .usage(vk::BufferUsageFlags::VERTEX_BUFFER)
    .sharing_mode(vk::SharingMode::EXCLUSIVE);
let buffer = unsafe { device.create_buffer(&buffer_info, None)? };

// Get memory requirements
let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };

// Allocate memory
let allocation = allocator.allocate(&AllocationCreateDesc {
    name: "vertex buffer",
    requirements,
    location: MemoryLocation::CpuToGpu,
    linear: true, // true for buffers, false for images
    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
})?;

// Bind memory
unsafe {
    device.bind_buffer_memory(buffer, allocation.memory(), allocation.offset())?;
}
```

### Image Allocation
```rust
let image_info = vk::ImageCreateInfo::default()
    .image_type(vk::ImageType::TYPE_2D)
    .format(vk::Format::R8G8B8A8_UNORM)
    .extent(vk::Extent3D { width, height, depth: 1 })
    .mip_levels(1)
    .array_layers(1)
    .samples(vk::SampleCountFlags::TYPE_1)
    .tiling(vk::ImageTiling::OPTIMAL)
    .usage(vk::ImageUsageFlags::SAMPLED)
    .sharing_mode(vk::SharingMode::EXCLUSIVE)
    .initial_layout(vk::ImageLayout::UNDEFINED);

let image = unsafe { device.create_image(&image_info, None)? };
let requirements = unsafe { device.get_image_memory_requirements(image) };

let allocation = allocator.allocate(&AllocationCreateDesc {
    name: "texture",
    requirements,
    location: MemoryLocation::GpuOnly,
    linear: false, // false for images
    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
})?;

unsafe {
    device.bind_image_memory(image, allocation.memory(), allocation.offset())?;
}
```

## Uploading Data

### CPU to GPU Transfer
```rust
// Map memory and copy data
if let Some(mapped_ptr) = allocation.mapped_ptr() {
    unsafe {
        std::ptr::copy_nonoverlapping(
            data.as_ptr(),
            mapped_ptr.as_ptr() as *mut u8,
            data.len()
        );
    }
} else {
    // For non-host-visible memory, use staging buffer
}
```

## Resource Management

### Deallocation
```rust
// Free allocation (must happen before destroying buffer/image)
allocator.free(allocation)?;

// Destroy Vulkan resource
unsafe {
    device.destroy_buffer(buffer, None);
}
```

### RAII Wrapper Pattern
```rust
struct BufferResource {
    buffer: vk::Buffer,
    allocation: Option<Allocation>,
}

impl BufferResource {
    fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        if let Some(allocation) = self.allocation.take() {
            allocator.free(allocation).unwrap();
        }
        unsafe {
            device.destroy_buffer(self.buffer, None);
        }
    }
}
```

## Allocation Strategies

### AllocationScheme
```rust
// Managed by gpu-allocator (default)
AllocationScheme::GpuAllocatorManaged

// Dedicated allocation (entire memory block for this resource)
AllocationScheme::DedicatedBuffer(buffer)
AllocationScheme::DedicatedImage(image)
```

## Performance Tips
- Use `MemoryLocation::GpuOnly` for frequently accessed GPU resources
- Use `MemoryLocation::CpuToGpu` for dynamic/streaming data
- Prefer large allocations over many small ones
- Reuse allocations when possible
- Use dedicated allocations for large resources (>256MB)
- Profile memory usage with debug settings enabled

## Debug Features
```rust
let allocator = Allocator::new(&AllocatorCreateDesc {
    // ... other fields ...
    debug_settings: gpu_allocator::AllocatorDebugSettings {
        log_memory_information: true,
        log_allocations: true,
        log_frees: true,
        log_stack_traces: false,
    },
    // ... other fields ...
})?;
```

## Common Issues
- **Alignment**: gpu-allocator respects `requirements.alignment` automatically
- **Memory types**: Let gpu-allocator choose via `MemoryLocation`
- **Cleanup order**: Free allocations before destroying allocator
- **Concurrent access**: Wrap allocator in `Arc<Mutex<Allocator>>` for multi-threaded use
