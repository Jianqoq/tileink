//! Allocation + host-coherent upload cost; excludes GPU rendering and presentation.
use ash::vk;
use criterion::Criterion;
use std::hint::black_box;
use std::time::Duration;

use super::Result;
#[allow(dead_code)]
#[path = "../../src/native/runtime/vulkan/compute_memory.rs"]
mod compute_memory;
#[path = "../../src/native/runtime/vulkan/staging.rs"]
mod staging;
#[path = "../../src/native/runtime/vulkan/upload.rs"]
mod upload;

pub fn benchmark(c: &mut Criterion) {
    let identity = std::env::var("TILEINK_NATIVE_GPU").expect("pin the GPU LUID");
    let entry = unsafe { ash::Entry::load().unwrap() };
    let instance = unsafe {
        entry
            .create_instance(
                &vk::InstanceCreateInfo::default().application_info(
                    &vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1),
                ),
                None,
            )
            .unwrap()
    };
    let physical = unsafe { instance.enumerate_physical_devices().unwrap() }
        .into_iter()
        .find(|&physical| {
            let mut id = vk::PhysicalDeviceIDProperties::default();
            let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut id);
            unsafe {
                instance.get_physical_device_properties2(physical, &mut properties);
            }
            id.device_luid_valid == vk::TRUE
                && id
                    .device_luid
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
                    == identity
        })
        .expect("requested GPU unavailable");
    let family = unsafe { instance.get_physical_device_queue_family_properties(physical) }
        .iter()
        .position(|p| p.queue_flags.contains(vk::QueueFlags::COMPUTE))
        .unwrap() as u32;
    let queues = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(&[1.0])];
    let device = unsafe {
        instance
            .create_device(
                physical,
                &vk::DeviceCreateInfo::default().queue_create_infos(&queues),
                None,
            )
            .unwrap()
    };
    let memory = unsafe { instance.get_physical_device_memory_properties(physical) };
    let bytes = vec![37u8; 17 * 1024 * 1024];
    let mut group = c.benchmark_group("vulkan_staging_reuse");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    for reuse in [false, true] {
        let mut cached = None;
        group.bench_function(if reuse { "reuse" } else { "allocate" }, |b| {
            b.iter(|| {
                // Cross a capacity boundary and shrink, as Replay dimensions change.
                for size in [15, 17, 16] {
                    let mut plan = upload::Upload::new();
                    plan.push(black_box(&bytes[..size * 1024 * 1024]), 1)
                        .unwrap();
                    if reuse {
                        let storage =
                            staging::Staging::prepare(&device, &memory, &plan, &mut cached)
                                .unwrap();
                        black_box(&storage);
                        cached = Some(storage);
                    } else {
                        let storage = compute_memory::Arena::new(
                            &device,
                            &memory,
                            &[plan.len() as u64],
                            vk::BufferUsageFlags::TRANSFER_SRC
                                | vk::BufferUsageFlags::UNIFORM_BUFFER,
                            vk::MemoryPropertyFlags::HOST_VISIBLE
                                | vk::MemoryPropertyFlags::HOST_COHERENT,
                        )
                        .unwrap();
                        storage.write(&plan).unwrap();
                        black_box(&storage);
                    }
                }
            });
        });
        drop(cached);
    }
    group.finish();
    unsafe {
        device.destroy_device(None);
        instance.destroy_instance(None);
    }
}
