use ash::vk;
use super::buffer::{BufferResource, get_buffer_device_address};
use super::device::VulkanDevice;

pub struct AccelStructures {
    pub cornell_blas: vk::AccelerationStructureKHR,
    pub cornell_blas_buffer: BufferResource,
    pub sphere_blas: vk::AccelerationStructureKHR,
    pub sphere_blas_buffer: BufferResource,
    pub tlas: vk::AccelerationStructureKHR,
    pub tlas_buffer: BufferResource,
    pub tlas_scratch_buffer: BufferResource,
    pub vertex_buffer: BufferResource,
    pub index_buffer: BufferResource,
    pub aabb_buffer: BufferResource,
    pub instance_buffer: BufferResource,
}

impl AccelStructures {
    pub fn build(
        vk_dev: &VulkanDevice,
        command_pool: vk::CommandPool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = &vk_dev.device;
        let mem_props = vk_dev.memory_properties;
        let accel = &vk_dev.accel_structure;

        // -- Cornell Box geometry: 5 quads (10 triangles) --
        // Box spans [-1, 1] in X, [-1, 1] in Y, [-1, 1] in Z
        // Front face (z=1) is open (camera looks in from there)

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Vertex {
            pos: [f32; 3],
        }

        let vertices = [
            // Floor (white) - primitives 0, 1
            Vertex { pos: [-1.0, -1.0, -1.0] }, // 0
            Vertex { pos: [ 1.0, -1.0, -1.0] }, // 1
            Vertex { pos: [ 1.0, -1.0,  1.0] }, // 2
            Vertex { pos: [-1.0, -1.0,  1.0] }, // 3

            // Ceiling (white) - primitives 2, 3
            Vertex { pos: [-1.0,  1.0, -1.0] }, // 4
            Vertex { pos: [ 1.0,  1.0, -1.0] }, // 5
            Vertex { pos: [ 1.0,  1.0,  1.0] }, // 6
            Vertex { pos: [-1.0,  1.0,  1.0] }, // 7

            // Back wall (white) - primitives 4, 5
            Vertex { pos: [-1.0, -1.0, -1.0] }, // 8
            Vertex { pos: [ 1.0, -1.0, -1.0] }, // 9
            Vertex { pos: [ 1.0,  1.0, -1.0] }, // 10
            Vertex { pos: [-1.0,  1.0, -1.0] }, // 11

            // Left wall (red) - primitives 6, 7
            Vertex { pos: [-1.0, -1.0, -1.0] }, // 12
            Vertex { pos: [-1.0, -1.0,  1.0] }, // 13
            Vertex { pos: [-1.0,  1.0,  1.0] }, // 14
            Vertex { pos: [-1.0,  1.0, -1.0] }, // 15

            // Right wall (green) - primitives 8, 9
            Vertex { pos: [ 1.0, -1.0, -1.0] }, // 16
            Vertex { pos: [ 1.0, -1.0,  1.0] }, // 17
            Vertex { pos: [ 1.0,  1.0,  1.0] }, // 18
            Vertex { pos: [ 1.0,  1.0, -1.0] }, // 19
        ];

        let indices: [u32; 30] = [
            // Floor
            0, 2, 1,
            0, 3, 2,
            // Ceiling
            4, 5, 6,
            4, 6, 7,
            // Back wall
            8, 10, 9,
            8, 11, 10,
            // Left wall
            12, 14, 13,
            12, 15, 14,
            // Right wall
            16, 17, 18,
            16, 18, 19,
        ];

        let vertex_count = vertices.len();
        let vertex_stride = std::mem::size_of::<Vertex>();

        let vertex_buffer = BufferResource::new(
            device,
            mem_props,
            (vertex_stride * vertex_count) as u64,
            vk::BufferUsageFlags::VERTEX_BUFFER
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );
        vertex_buffer.store(&vertices, device);

        let index_buffer = BufferResource::new(
            device,
            mem_props,
            (std::mem::size_of::<u32>() * indices.len()) as u64,
            vk::BufferUsageFlags::INDEX_BUFFER
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );
        index_buffer.store(&indices, device);

        // -- Build Cornell BLAS --
        let geometry = vk::AccelerationStructureGeometryKHR::default()
            .geometry_type(vk::GeometryTypeKHR::TRIANGLES)
            .geometry(vk::AccelerationStructureGeometryDataKHR {
                triangles: vk::AccelerationStructureGeometryTrianglesDataKHR::default()
                    .vertex_data(vk::DeviceOrHostAddressConstKHR {
                        device_address: get_buffer_device_address(device, vertex_buffer.buffer),
                    })
                    .max_vertex(vertex_count as u32 - 1)
                    .vertex_stride(vertex_stride as u64)
                    .vertex_format(vk::Format::R32G32B32_SFLOAT)
                    .index_data(vk::DeviceOrHostAddressConstKHR {
                        device_address: get_buffer_device_address(device, index_buffer.buffer),
                    })
                    .index_type(vk::IndexType::UINT32),
            })
            .flags(vk::GeometryFlagsKHR::OPAQUE);

        let primitive_count = indices.len() as u32 / 3;
        let build_range = vk::AccelerationStructureBuildRangeInfoKHR::default()
            .primitive_count(primitive_count)
            .primitive_offset(0)
            .first_vertex(0)
            .transform_offset(0);

        let geometries = [geometry];
        let mut build_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .flags(
                vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE
                    | vk::BuildAccelerationStructureFlagsKHR::ALLOW_DATA_ACCESS,
            )
            .geometries(&geometries)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL);

        let mut size_info = vk::AccelerationStructureBuildSizesInfoKHR::default();
        unsafe {
            accel.get_acceleration_structure_build_sizes(
                vk::AccelerationStructureBuildTypeKHR::DEVICE,
                &build_info,
                &[primitive_count],
                &mut size_info,
            );
        }

        let cornell_blas_buffer = BufferResource::new(
            device,
            mem_props,
            size_info.acceleration_structure_size,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        let blas_create = vk::AccelerationStructureCreateInfoKHR::default()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .size(size_info.acceleration_structure_size)
            .buffer(cornell_blas_buffer.buffer);

        let cornell_blas = unsafe { accel.create_acceleration_structure(&blas_create, None)? };
        build_info.dst_acceleration_structure = cornell_blas;

        let scratch_buffer = BufferResource::new(
            device,
            mem_props,
            size_info.build_scratch_size,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS | vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        build_info.scratch_data = vk::DeviceOrHostAddressKHR {
            device_address: get_buffer_device_address(device, scratch_buffer.buffer),
        };

        execute_one_time(device, vk_dev.queue, command_pool, |cmd| {
            unsafe {
                accel.cmd_build_acceleration_structures(cmd, &[build_info], &[&[build_range]]);
            }
        })?;
        scratch_buffer.destroy(device);

        // -- Build Sphere AABB BLAS --
        // AABB centered at origin, radius 0.3 (will be transformed via instance transform)
        let r: f32 = 0.3;
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct AabbData {
            min: [f32; 3],
            max: [f32; 3],
        }
        let aabb = AabbData {
            min: [-r, -r, -r],
            max: [r, r, r],
        };

        let aabb_buffer = BufferResource::new(
            device,
            mem_props,
            std::mem::size_of::<AabbData>() as u64,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );
        aabb_buffer.store(
            unsafe {
                std::slice::from_raw_parts(&aabb as *const AabbData as *const u8, std::mem::size_of::<AabbData>())
            },
            device,
        );

        let sphere_geometry = vk::AccelerationStructureGeometryKHR::default()
            .geometry_type(vk::GeometryTypeKHR::AABBS)
            .geometry(vk::AccelerationStructureGeometryDataKHR {
                aabbs: vk::AccelerationStructureGeometryAabbsDataKHR::default()
                    .data(vk::DeviceOrHostAddressConstKHR {
                        device_address: get_buffer_device_address(device, aabb_buffer.buffer),
                    })
                    .stride(std::mem::size_of::<AabbData>() as u64),
            })
            .flags(vk::GeometryFlagsKHR::empty()); // Not opaque - glass is transparent

        let sphere_build_range = vk::AccelerationStructureBuildRangeInfoKHR::default()
            .primitive_count(1)
            .primitive_offset(0)
            .first_vertex(0)
            .transform_offset(0);

        let sphere_geometries = [sphere_geometry];
        let mut sphere_build_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .geometries(&sphere_geometries)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL);

        let mut sphere_size_info = vk::AccelerationStructureBuildSizesInfoKHR::default();
        unsafe {
            accel.get_acceleration_structure_build_sizes(
                vk::AccelerationStructureBuildTypeKHR::DEVICE,
                &sphere_build_info,
                &[1],
                &mut sphere_size_info,
            );
        }

        let sphere_blas_buffer = BufferResource::new(
            device,
            mem_props,
            sphere_size_info.acceleration_structure_size,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        let sphere_blas_create = vk::AccelerationStructureCreateInfoKHR::default()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .size(sphere_size_info.acceleration_structure_size)
            .buffer(sphere_blas_buffer.buffer);

        let sphere_blas = unsafe { accel.create_acceleration_structure(&sphere_blas_create, None)? };
        sphere_build_info.dst_acceleration_structure = sphere_blas;

        let sphere_scratch = BufferResource::new(
            device,
            mem_props,
            sphere_size_info.build_scratch_size,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS | vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        sphere_build_info.scratch_data = vk::DeviceOrHostAddressKHR {
            device_address: get_buffer_device_address(device, sphere_scratch.buffer),
        };

        execute_one_time(device, vk_dev.queue, command_pool, |cmd| {
            unsafe {
                accel.cmd_build_acceleration_structures(
                    cmd,
                    &[sphere_build_info],
                    &[&[sphere_build_range]],
                );
            }
        })?;
        sphere_scratch.destroy(device);

        // -- Build TLAS with 2 instances --
        let cornell_blas_address = unsafe {
            let addr_info = vk::AccelerationStructureDeviceAddressInfoKHR::default()
                .acceleration_structure(cornell_blas);
            accel.get_acceleration_structure_device_address(&addr_info)
        };

        let sphere_blas_address = unsafe {
            let addr_info = vk::AccelerationStructureDeviceAddressInfoKHR::default()
                .acceleration_structure(sphere_blas);
            accel.get_acceleration_structure_device_address(&addr_info)
        };

        let identity: [f32; 12] = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
        ];

        // Instance 0: Cornell box (SBT offset 0 -> hits group 1 = opaque)
        let cornell_instance = vk::AccelerationStructureInstanceKHR {
            transform: vk::TransformMatrixKHR { matrix: identity },
            instance_custom_index_and_mask: vk::Packed24_8::new(0, 0xFF),
            instance_shader_binding_table_record_offset_and_flags: vk::Packed24_8::new(
                0,
                vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE.as_raw() as u8,
            ),
            acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
                device_handle: cornell_blas_address,
            },
        };

        // Instance 1: Sphere (SBT offset 1 -> hits group 2 = procedural glass)
        // Initial transform places sphere at orbit start position
        let sphere_instance = vk::AccelerationStructureInstanceKHR {
            transform: vk::TransformMatrixKHR { matrix: identity },
            instance_custom_index_and_mask: vk::Packed24_8::new(1, 0xFF),
            instance_shader_binding_table_record_offset_and_flags: vk::Packed24_8::new(
                1,
                vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE.as_raw() as u8,
            ),
            acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
                device_handle: sphere_blas_address,
            },
        };

        let instances = [cornell_instance, sphere_instance];
        let instance_size = std::mem::size_of::<vk::AccelerationStructureInstanceKHR>();

        let instance_buffer = BufferResource::new(
            device,
            mem_props,
            (instance_size * instances.len()) as u64,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );
        instance_buffer.store(&instances, device);

        let tlas_geometry = vk::AccelerationStructureGeometryKHR::default()
            .geometry_type(vk::GeometryTypeKHR::INSTANCES)
            .geometry(vk::AccelerationStructureGeometryDataKHR {
                instances: vk::AccelerationStructureGeometryInstancesDataKHR::default()
                    .array_of_pointers(false)
                    .data(vk::DeviceOrHostAddressConstKHR {
                        device_address: get_buffer_device_address(device, instance_buffer.buffer),
                    }),
            });

        let instance_count = instances.len() as u32;
        let tlas_build_range = vk::AccelerationStructureBuildRangeInfoKHR::default()
            .primitive_count(instance_count)
            .primitive_offset(0)
            .first_vertex(0)
            .transform_offset(0);

        let tlas_geometries = [tlas_geometry];
        let mut tlas_build_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .flags(
                vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_BUILD
                    | vk::BuildAccelerationStructureFlagsKHR::ALLOW_UPDATE,
            )
            .geometries(&tlas_geometries)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL);

        let mut tlas_size_info = vk::AccelerationStructureBuildSizesInfoKHR::default();
        unsafe {
            accel.get_acceleration_structure_build_sizes(
                vk::AccelerationStructureBuildTypeKHR::DEVICE,
                &tlas_build_info,
                &[instance_count],
                &mut tlas_size_info,
            );
        }

        let tlas_buffer = BufferResource::new(
            device,
            mem_props,
            tlas_size_info.acceleration_structure_size,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        let tlas_create = vk::AccelerationStructureCreateInfoKHR::default()
            .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
            .size(tlas_size_info.acceleration_structure_size)
            .buffer(tlas_buffer.buffer);

        let tlas = unsafe { accel.create_acceleration_structure(&tlas_create, None)? };
        tlas_build_info.dst_acceleration_structure = tlas;

        // Keep scratch buffer alive for per-frame rebuilds
        let tlas_scratch_buffer = BufferResource::new(
            device,
            mem_props,
            tlas_size_info.build_scratch_size,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS | vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        tlas_build_info.scratch_data = vk::DeviceOrHostAddressKHR {
            device_address: get_buffer_device_address(device, tlas_scratch_buffer.buffer),
        };

        execute_one_time(device, vk_dev.queue, command_pool, |cmd| {
            unsafe {
                let barrier = vk::MemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::ACCELERATION_STRUCTURE_WRITE_KHR);
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::ACCELERATION_STRUCTURE_BUILD_KHR,
                    vk::DependencyFlags::empty(),
                    &[barrier],
                    &[],
                    &[],
                );
                accel.cmd_build_acceleration_structures(
                    cmd,
                    &[tlas_build_info],
                    &[&[tlas_build_range]],
                );
            }
        })?;

        log::info!("Acceleration structures built (Cornell BLAS + Sphere BLAS + TLAS)");

        Ok(AccelStructures {
            cornell_blas,
            cornell_blas_buffer,
            sphere_blas,
            sphere_blas_buffer,
            tlas,
            tlas_buffer,
            tlas_scratch_buffer,
            vertex_buffer,
            index_buffer,
            aabb_buffer,
            instance_buffer,
        })
    }

    /// Update sphere instance transform and rebuild TLAS in command buffer
    pub fn cmd_rebuild_tlas(
        &self,
        cmd: vk::CommandBuffer,
        vk_dev: &VulkanDevice,
    ) {
        let accel = &vk_dev.accel_structure;

        let tlas_geometry = vk::AccelerationStructureGeometryKHR::default()
            .geometry_type(vk::GeometryTypeKHR::INSTANCES)
            .geometry(vk::AccelerationStructureGeometryDataKHR {
                instances: vk::AccelerationStructureGeometryInstancesDataKHR::default()
                    .array_of_pointers(false)
                    .data(vk::DeviceOrHostAddressConstKHR {
                        device_address: get_buffer_device_address(
                            &vk_dev.device,
                            self.instance_buffer.buffer,
                        ),
                    }),
            });

        let tlas_build_range = vk::AccelerationStructureBuildRangeInfoKHR::default()
            .primitive_count(2)
            .primitive_offset(0)
            .first_vertex(0)
            .transform_offset(0);

        let tlas_geometries = [tlas_geometry];
        let tlas_build_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .flags(
                vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_BUILD
                    | vk::BuildAccelerationStructureFlagsKHR::ALLOW_UPDATE,
            )
            .geometries(&tlas_geometries)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
            .dst_acceleration_structure(self.tlas)
            .scratch_data(vk::DeviceOrHostAddressKHR {
                device_address: get_buffer_device_address(
                    &vk_dev.device,
                    self.tlas_scratch_buffer.buffer,
                ),
            });

        unsafe {
            accel.cmd_build_acceleration_structures(
                cmd,
                &[tlas_build_info],
                &[&[tlas_build_range]],
            );
        }
    }

    /// Update the sphere instance transform in the instance buffer
    pub fn update_sphere_transform(&self, device: &ash::Device, center: [f32; 3]) {
        // We need to write just the sphere instance (index 1) transform
        // The transform is a 3x4 row-major matrix (translation only)
        let transform: [f32; 12] = [
            1.0, 0.0, 0.0, center[0],
            0.0, 1.0, 0.0, center[1],
            0.0, 0.0, 1.0, center[2],
        ];

        let instance_size = std::mem::size_of::<vk::AccelerationStructureInstanceKHR>();
        let offset = instance_size; // Instance 1 starts at offset of 1 instance

        unsafe {
            let ptr = device
                .map_memory(
                    self.instance_buffer.memory,
                    offset as u64,
                    instance_size as u64,
                    vk::MemoryMapFlags::empty(),
                )
                .unwrap() as *mut u8;

            // Write just the transform (first 48 bytes of the instance struct)
            std::ptr::copy_nonoverlapping(
                transform.as_ptr() as *const u8,
                ptr,
                48, // 12 floats * 4 bytes
            );

            device.unmap_memory(self.instance_buffer.memory);
        }
    }

    pub fn destroy(&self, vk_dev: &VulkanDevice) {
        let device = &vk_dev.device;
        let accel = &vk_dev.accel_structure;
        unsafe {
            accel.destroy_acceleration_structure(self.tlas, None);
            accel.destroy_acceleration_structure(self.sphere_blas, None);
            accel.destroy_acceleration_structure(self.cornell_blas, None);
        }
        self.tlas_buffer.destroy(device);
        self.tlas_scratch_buffer.destroy(device);
        self.sphere_blas_buffer.destroy(device);
        self.cornell_blas_buffer.destroy(device);
        self.instance_buffer.destroy(device);
        self.aabb_buffer.destroy(device);
        self.index_buffer.destroy(device);
        self.vertex_buffer.destroy(device);
    }
}

fn execute_one_time(
    device: &ash::Device,
    queue: vk::Queue,
    pool: vk::CommandPool,
    record: impl FnOnce(vk::CommandBuffer),
) -> Result<(), vk::Result> {
    unsafe {
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);

        let cmd = device.allocate_command_buffers(&alloc_info)?[0];

        device.begin_command_buffer(
            cmd,
            &vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
        )?;

        record(cmd);

        device.end_command_buffer(cmd)?;

        let submit = vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cmd));
        device.queue_submit(queue, &[submit], vk::Fence::null())?;
        device.queue_wait_idle(queue)?;
        device.free_command_buffers(pool, &[cmd]);
    }
    Ok(())
}
