//! Allocation + upload only; no GPU execution or presentation is timed.
use super::Result;
use criterion::Criterion;
use std::{hint::black_box, time::Duration};
use windows::Win32::Graphics::{Direct3D::D3D_FEATURE_LEVEL_12_0, Direct3D12::*, Dxgi::*};

#[allow(dead_code)]
#[path = "../../src/native/runtime/dx12/buffer.rs"]
mod buffer;
#[path = "../../src/native/runtime/dx12/staging.rs"]
mod staging;

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
        let mut cached = Vec::new();
        group.bench_function(if reuse { "reuse" } else { "allocate" }, |b| {
            b.iter(|| {
                for size in [15, 17, 16] {
                    let contents = black_box(&bytes[..size * 1024 * 1024]);
                    if reuse {
                        let resource = staging::prepare(
                            &device,
                            contents,
                            &mut std::mem::take(&mut cached).into_iter(),
                        )
                        .unwrap();
                        black_box(&resource);
                        cached.push(resource);
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
}
