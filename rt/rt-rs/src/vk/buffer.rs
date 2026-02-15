use ash::vk;

pub struct BufferResource {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub size: vk::DeviceSize,
}

impl BufferResource {
    pub fn new(
        device: &ash::Device,
        mem_props: vk::PhysicalDeviceMemoryProperties,
        size: vk::DeviceSize,
        usage: vk::BufferUsageFlags,
        memory_flags: vk::MemoryPropertyFlags,
    ) -> Self {
        unsafe {
            let buffer_info = vk::BufferCreateInfo::default()
                .size(size)
                .usage(usage)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);

            let buffer = device.create_buffer(&buffer_info, None).unwrap();
            let mem_reqs = device.get_buffer_memory_requirements(buffer);

            let memory_index = find_memory_type(mem_props, mem_reqs.memory_type_bits, memory_flags);

            let mut alloc_flags = vk::MemoryAllocateFlagsInfo::default()
                .flags(vk::MemoryAllocateFlags::DEVICE_ADDRESS);

            let mut alloc_info = vk::MemoryAllocateInfo::default()
                .allocation_size(mem_reqs.size)
                .memory_type_index(memory_index);

            if usage.contains(vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS) {
                alloc_info = alloc_info.push_next(&mut alloc_flags);
            }

            let memory = device.allocate_memory(&alloc_info, None).unwrap();
            device.bind_buffer_memory(buffer, memory, 0).unwrap();

            BufferResource {
                buffer,
                memory,
                size,
            }
        }
    }

    pub fn store<T: Copy>(&self, data: &[T], device: &ash::Device) {
        unsafe {
            let byte_size = std::mem::size_of_val(data) as u64;
            assert!(self.size >= byte_size);
            let ptr = device
                .map_memory(self.memory, 0, byte_size, vk::MemoryMapFlags::empty())
                .unwrap();
            std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, ptr as *mut u8, byte_size as usize);
            device.unmap_memory(self.memory);
        }
    }

    pub fn destroy(&self, device: &ash::Device) {
        unsafe {
            device.destroy_buffer(self.buffer, None);
            device.free_memory(self.memory, None);
        }
    }
}

pub fn get_buffer_device_address(device: &ash::Device, buffer: vk::Buffer) -> u64 {
    let info = vk::BufferDeviceAddressInfo::default().buffer(buffer);
    unsafe { device.get_buffer_device_address(&info) }
}

pub fn find_memory_type(
    mem_props: vk::PhysicalDeviceMemoryProperties,
    mut type_bits: u32,
    required: vk::MemoryPropertyFlags,
) -> u32 {
    for i in 0..mem_props.memory_type_count {
        if (type_bits & 1) == 1
            && (mem_props.memory_types[i as usize].property_flags & required) == required
        {
            return i;
        }
        type_bits >>= 1;
    }
    panic!("Failed to find suitable memory type");
}
