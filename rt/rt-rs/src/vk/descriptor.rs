use ash::vk;

pub struct Descriptors {
    pub pool: vk::DescriptorPool,
    pub set_layout: vk::DescriptorSetLayout,
    pub set: vk::DescriptorSet,
}

impl Descriptors {
    pub fn new(device: &ash::Device) -> Result<Self, vk::Result> {
        // Layout:
        //   binding 0 = TLAS (raygen + closest hit + miss)
        //   binding 1 = storage image (raygen)
        //   binding 2 = camera UBO (raygen)
        //   binding 3 = scene UBO (raygen + closest hit + miss)
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
                .stage_flags(
                    vk::ShaderStageFlags::RAYGEN_KHR
                        | vk::ShaderStageFlags::CLOSEST_HIT_KHR
                        | vk::ShaderStageFlags::MISS_KHR,
                ),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR),
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR),
            vk::DescriptorSetLayoutBinding::default()
                .binding(3)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .stage_flags(
                    vk::ShaderStageFlags::RAYGEN_KHR
                        | vk::ShaderStageFlags::CLOSEST_HIT_KHR
                        | vk::ShaderStageFlags::MISS_KHR
                        | vk::ShaderStageFlags::INTERSECTION_KHR,
                ),
        ];

        let layout_info =
            vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
        let set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None)? };

        let pool_sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
                descriptor_count: 1,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_IMAGE,
                descriptor_count: 1,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER,
                descriptor_count: 2, // camera + scene
            },
        ];

        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .pool_sizes(&pool_sizes)
            .max_sets(1);
        let pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        let layouts = [set_layout];
        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&layouts);
        let set = unsafe { device.allocate_descriptor_sets(&alloc_info)?[0] };

        Ok(Descriptors {
            pool,
            set_layout,
            set,
        })
    }

    pub fn update(
        &self,
        device: &ash::Device,
        tlas: vk::AccelerationStructureKHR,
        image_view: vk::ImageView,
        camera_buffer: vk::Buffer,
        camera_buffer_size: u64,
        scene_buffer: vk::Buffer,
        scene_buffer_size: u64,
    ) {
        let accel_structs = [tlas];
        let mut accel_info = vk::WriteDescriptorSetAccelerationStructureKHR::default()
            .acceleration_structures(&accel_structs);

        let mut accel_write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(0)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
            .push_next(&mut accel_info);
        accel_write.descriptor_count = 1;

        let image_infos = [vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::GENERAL)
            .image_view(image_view)];

        let image_write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(1)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
            .image_info(&image_infos);

        let camera_buffer_infos = [vk::DescriptorBufferInfo::default()
            .buffer(camera_buffer)
            .offset(0)
            .range(camera_buffer_size)];

        let camera_write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(2)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .buffer_info(&camera_buffer_infos);

        let scene_buffer_infos = [vk::DescriptorBufferInfo::default()
            .buffer(scene_buffer)
            .offset(0)
            .range(scene_buffer_size)];

        let scene_write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(3)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .buffer_info(&scene_buffer_infos);

        unsafe {
            device.update_descriptor_sets(
                &[accel_write, image_write, camera_write, scene_write],
                &[],
            );
        }
    }

    pub fn destroy(&self, device: &ash::Device) {
        unsafe {
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
        }
    }
}
