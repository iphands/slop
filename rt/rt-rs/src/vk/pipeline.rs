use ash::vk;

use super::buffer::BufferResource;
use super::device::VulkanDevice;

pub struct RtPipeline {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub sbt_buffer: BufferResource,
    pub raygen_region: vk::StridedDeviceAddressRegionKHR,
    pub miss_region: vk::StridedDeviceAddressRegionKHR,
    pub hit_region: vk::StridedDeviceAddressRegionKHR,
    pub call_region: vk::StridedDeviceAddressRegionKHR,
}

impl RtPipeline {
    pub fn new(
        vk_dev: &VulkanDevice,
        descriptor_set_layout: vk::DescriptorSetLayout,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = &vk_dev.device;

        // Load precompiled SPIR-V shaders
        let raygen_spv = include_bytes!("../shaders/raygen.spv");
        let chit_opaque_spv = include_bytes!("../shaders/closesthit_opaque.spv");
        let chit_glass_spv = include_bytes!("../shaders/closesthit_glass.spv");
        let sphere_int_spv = include_bytes!("../shaders/sphere.spv");
        let miss_spv = include_bytes!("../shaders/miss.spv");
        let shadow_miss_spv = include_bytes!("../shaders/shadow_miss.spv");

        let raygen_module = create_shader_module(device, raygen_spv)?;
        let chit_opaque_module = create_shader_module(device, chit_opaque_spv)?;
        let chit_glass_module = create_shader_module(device, chit_glass_spv)?;
        let sphere_int_module = create_shader_module(device, sphere_int_spv)?;
        let miss_module = create_shader_module(device, miss_spv)?;
        let shadow_miss_module = create_shader_module(device, shadow_miss_spv)?;

        let entry_point = c"main";

        let stages = [
            // 0: raygen
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::RAYGEN_KHR)
                .module(raygen_module)
                .name(entry_point),
            // 1: opaque closest hit
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::CLOSEST_HIT_KHR)
                .module(chit_opaque_module)
                .name(entry_point),
            // 2: glass closest hit
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::CLOSEST_HIT_KHR)
                .module(chit_glass_module)
                .name(entry_point),
            // 3: sphere intersection
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::INTERSECTION_KHR)
                .module(sphere_int_module)
                .name(entry_point),
            // 4: miss
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::MISS_KHR)
                .module(miss_module)
                .name(entry_point),
            // 5: shadow miss
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::MISS_KHR)
                .module(shadow_miss_module)
                .name(entry_point),
        ];

        let groups = [
            // Group 0: raygen (GENERAL)
            vk::RayTracingShaderGroupCreateInfoKHR::default()
                .ty(vk::RayTracingShaderGroupTypeKHR::GENERAL)
                .general_shader(0)
                .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR),
            // Group 1: opaque hit (TRIANGLES_HIT_GROUP)
            vk::RayTracingShaderGroupCreateInfoKHR::default()
                .ty(vk::RayTracingShaderGroupTypeKHR::TRIANGLES_HIT_GROUP)
                .general_shader(vk::SHADER_UNUSED_KHR)
                .closest_hit_shader(1)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR),
            // Group 2: procedural glass hit (PROCEDURAL_HIT_GROUP)
            vk::RayTracingShaderGroupCreateInfoKHR::default()
                .ty(vk::RayTracingShaderGroupTypeKHR::PROCEDURAL_HIT_GROUP)
                .general_shader(vk::SHADER_UNUSED_KHR)
                .closest_hit_shader(2)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(3),
            // Group 3: miss (GENERAL)
            vk::RayTracingShaderGroupCreateInfoKHR::default()
                .ty(vk::RayTracingShaderGroupTypeKHR::GENERAL)
                .general_shader(4)
                .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR),
            // Group 4: shadow miss (GENERAL)
            vk::RayTracingShaderGroupCreateInfoKHR::default()
                .ty(vk::RayTracingShaderGroupTypeKHR::GENERAL)
                .general_shader(5)
                .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR),
        ];

        let layouts = [descriptor_set_layout];
        let layout_info =
            vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts);
        let pipeline_layout = unsafe { device.create_pipeline_layout(&layout_info, None)? };

        let rt_pipeline_info = vk::RayTracingPipelineCreateInfoKHR::default()
            .stages(&stages)
            .groups(&groups)
            .max_pipeline_ray_recursion_depth(31)
            .layout(pipeline_layout);

        let pipeline = unsafe {
            vk_dev
                .rt_pipeline
                .create_ray_tracing_pipelines(
                    vk::DeferredOperationKHR::null(),
                    vk::PipelineCache::null(),
                    &[rt_pipeline_info],
                    None,
                )
                .map_err(|(_, err)| err)?[0]
        };

        unsafe {
            device.destroy_shader_module(raygen_module, None);
            device.destroy_shader_module(chit_opaque_module, None);
            device.destroy_shader_module(chit_glass_module, None);
            device.destroy_shader_module(sphere_int_module, None);
            device.destroy_shader_module(miss_module, None);
            device.destroy_shader_module(shadow_miss_module, None);
        }

        // Build Shader Binding Table
        // SBT layout: [raygen(1)] [hit_opaque(1), hit_glass(1)] [miss(1), shadow_miss(1)] [callable(0)]
        let handle_size = vk_dev.shader_group_handle_size;
        let base_alignment = vk_dev.shader_group_base_alignment;
        let handle_size_aligned = aligned_size(handle_size, base_alignment) as u64;
        let group_count = groups.len() as u32;

        let handles = unsafe {
            vk_dev.rt_pipeline.get_ray_tracing_shader_group_handles(
                pipeline,
                0,
                group_count,
                (group_count * handle_size) as usize,
            )?
        };

        // Regions:
        //   raygen: 1 entry
        //   hit: 2 entries (opaque + glass)
        //   miss: 2 entries (primary miss + shadow miss)
        let raygen_count = 1u64;
        let hit_count = 2u64;
        let miss_count = 2u64;

        let raygen_size = raygen_count * handle_size_aligned;
        let hit_size = hit_count * handle_size_aligned;
        let miss_size = miss_count * handle_size_aligned;
        let total_size = raygen_size + hit_size + miss_size;

        let mut table_data = vec![0u8; total_size as usize];

        // Group order: 0=raygen, 1=hit_opaque, 2=hit_glass, 3=miss, 4=shadow_miss
        // SBT layout: [raygen | hit_opaque, hit_glass | miss, shadow_miss]

        // Raygen at offset 0
        let src = 0 * handle_size as usize;
        let dst = 0usize;
        table_data[dst..dst + handle_size as usize]
            .copy_from_slice(&handles[src..src + handle_size as usize]);

        // Hit opaque at raygen_size
        let src = 1 * handle_size as usize;
        let dst = raygen_size as usize;
        table_data[dst..dst + handle_size as usize]
            .copy_from_slice(&handles[src..src + handle_size as usize]);

        // Hit glass at raygen_size + handle_size_aligned
        let src = 2 * handle_size as usize;
        let dst = (raygen_size + handle_size_aligned) as usize;
        table_data[dst..dst + handle_size as usize]
            .copy_from_slice(&handles[src..src + handle_size as usize]);

        // Miss at raygen_size + hit_size
        let src = 3 * handle_size as usize;
        let dst = (raygen_size + hit_size) as usize;
        table_data[dst..dst + handle_size as usize]
            .copy_from_slice(&handles[src..src + handle_size as usize]);

        // Shadow miss at raygen_size + hit_size + handle_size_aligned
        let src = 4 * handle_size as usize;
        let dst = (raygen_size + hit_size + handle_size_aligned) as usize;
        table_data[dst..dst + handle_size as usize]
            .copy_from_slice(&handles[src..src + handle_size as usize]);

        let sbt_buffer = BufferResource::new(
            device,
            vk_dev.memory_properties,
            total_size,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );
        sbt_buffer.store(&table_data, device);

        let sbt_address =
            super::buffer::get_buffer_device_address(device, sbt_buffer.buffer);

        let raygen_region = vk::StridedDeviceAddressRegionKHR {
            device_address: sbt_address,
            stride: handle_size_aligned,
            size: raygen_size,
        };
        let hit_region = vk::StridedDeviceAddressRegionKHR {
            device_address: sbt_address + raygen_size,
            stride: handle_size_aligned,
            size: hit_size,
        };
        let miss_region = vk::StridedDeviceAddressRegionKHR {
            device_address: sbt_address + raygen_size + hit_size,
            stride: handle_size_aligned,
            size: miss_size,
        };
        let call_region = vk::StridedDeviceAddressRegionKHR::default();

        log::info!("Ray tracing pipeline and SBT created (5 groups, max recursion depth 31)");

        Ok(RtPipeline {
            pipeline,
            pipeline_layout,
            sbt_buffer,
            raygen_region,
            miss_region,
            hit_region,
            call_region,
        })
    }

    pub fn destroy(&self, device: &ash::Device) {
        unsafe {
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);
        }
        self.sbt_buffer.destroy(device);
    }
}

fn create_shader_module(
    device: &ash::Device,
    code: &[u8],
) -> Result<vk::ShaderModule, vk::Result> {
    let create_info = vk::ShaderModuleCreateInfo {
        s_type: vk::StructureType::SHADER_MODULE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: vk::ShaderModuleCreateFlags::empty(),
        code_size: code.len(),
        p_code: code.as_ptr() as *const u32,
        _marker: std::marker::PhantomData,
    };
    unsafe { device.create_shader_module(&create_info, None) }
}

fn aligned_size(value: u32, alignment: u32) -> u32 {
    (value + alignment - 1) & !(alignment - 1)
}
