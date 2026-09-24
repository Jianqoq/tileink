//! Allocation + upload only; no GPU execution or presentation is timed.
use super::Result;
use criterion::Criterion;
use std::{hint::black_box, time::Duration};
use windows::Win32::Graphics::{Direct3D::D3D_FEATURE_LEVEL_12_0, Direct3D12::*, Dxgi::*};

#[allow(dead_code)]
#[path = "../../src/native/runtime/dx12/buffer.rs"]
mod buffer;
#[path = "../../src/native/runtime/dx12/buffer_cache.rs"]
mod buffer_cache;
#[path = "../../src/native/runtime/dx12/staging.rs"]
mod staging;
#[path = "../../src/native/runtime/dx12/storage.rs"]
mod storage;

pub fn benchmark(c: &mut Criterion) {
    let identity = std::env::var("TILEINK_NATIVE_GPU").expect("pin the GPU LUID");
    let device: ID3D12Device = unsafe {
        let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).unwrap();
        let mut selected = None;
        for index in 0.. {
            let adapter = match factory.EnumAdapters1(index) {
                Ok(adapter) => adapter,
                Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(error) => panic!("adapter enumeration failed: {error}"),
            };
            let luid = adapter.GetDesc1().unwrap().AdapterLuid;
            let actual: String = [luid.LowPart.to_le_bytes(), luid.HighPart.to_le_bytes()]
                .concat()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            if actual == identity {
                selected = Some(adapter);
                break;
            }
        }
        let mut device = None;
        D3D12CreateDevice(
            &selected.expect("requested GPU unavailable"),
            D3D_FEATURE_LEVEL_12_0,
            &mut device,
        )
        .unwrap();
        device.unwrap()
    };
    let bytes = vec![37u8; 17 * 1024 * 1024];
    let mut group = c.benchmark_group("dx12_staging_reuse");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    for reuse in [false, true] {
        let mut cached = buffer_cache::Pool::default();
        group.bench_function(if reuse { "reuse" } else { "allocate" }, |b| {
            b.iter(|| {
                for size in [15, 17, 16] {
                    let contents = black_box(&bytes[..size * 1024 * 1024]);
                    if reuse {
                        let mut available = cached.acquire();
                        let resource = staging::prepare(&device, contents, &mut available).unwrap();
                        black_box(&resource);
                        cached.release_unused(available);
                        cached.retire(vec![resource]);
                    } else {
                        let resource = buffer::create(
                            &device,
                            contents.len(),
                            D3D12_HEAP_TYPE_UPLOAD,
                            D3D12_RESOURCE_STATE_GENERIC_READ,
                            D3D12_RESOURCE_FLAG_NONE,
                            Some(contents),
                        )
                        .unwrap();
                        black_box(&resource);
                    }
                }
            });
        });
    }
    group.finish();
    // Resize retires multiple slots at once before refilling the frame pipeline.
    // Compare the former last-retired-only policy with retaining every free slot.
    let mut group = c.benchmark_group("dx12_resize_pipeline_refill");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    for retain_all in [false, true] {
        let mut pool = buffer_cache::Pool::default();
        group.bench_function(if retain_all { "all_slots" } else { "last_slot" }, |b| {
            b.iter(|| {
                let mut pending = Vec::new();
                for _ in 0..2 {
                    let mut available = pool.acquire();
                    let mut uploads = Vec::new();
                    for size in [512, 9 * 1024 * 1024] {
                        uploads.push(
                            staging::prepare(&device, black_box(&bytes[..size]), &mut available)
                                .unwrap(),
                        );
                    }
                    pool.release_unused(available);
                    pending.push(uploads);
                }
                black_box(&pending);
                for uploads in pending {
                    if !retain_all {
                        pool.frames.clear();
                    }
                    pool.retire(uploads);
                }
            });
        });
    }
    group.finish();
    let mut group = c.benchmark_group("dx12_device_buffer_reuse");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    for reuse in [false, true] {
        let mut cached = buffer_cache::Pool::default();
        group.bench_function(if reuse { "reuse" } else { "allocate" }, |b| {
            b.iter(|| {
                let mut previous = cached.acquire();
                let mut next = Vec::new();
                for index in 0..32 {
                    let size = 512 * 1024 + (index % 3) * 256;
                    let resource = if reuse {
                        storage::prepare(&device, size, &mut previous).unwrap()
                    } else {
                        buffer::create(
                            &device,
                            size,
                            D3D12_HEAP_TYPE_DEFAULT,
                            D3D12_RESOURCE_STATE_COMMON,
                            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
                            None,
                        )
                        .unwrap()
                        .into()
                    };
                    next.push(resource);
                }
                black_box(&next);
                if reuse {
                    cached.release_unused(previous);
                    cached.retire(next);
                }
            });
        });
    }
    group.finish();
}
