//! Geometry count parity exercises original WGSL and an independent CPU tile count.
use super::{Result, reference};
use crate::{
    NativeBackend,
    native::runtime::{adapter::Adapter, compute::ComputeBatch},
    shared::{bounds::TileBbox, line::Line, path::PathRecord, scan_line::line_scanned_tile_count},
};

#[path = "scan_cases.rs"]
mod scan_cases;
use scan_cases::count_case;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_scan_count_matches_clipped_geometry_and_cpu_tile_count() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let dx12 = Adapter::new(NativeBackend::Dx12, &identity)?;
    let vulkan = Adapter::new(NativeBackend::Vulkan, &identity)?;
    let references = [
        reference::Reference::new(wgpu::Backends::DX12, &identity)?,
        reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
    ];
    let mut segments = vec![
        ([8.0, 0.0], [8.0, 48.0]),
        ([0.0, 8.0], [48.0, 8.0]),
        ([-8.0, 0.0], [-8.0, 48.0]),
        ([0.0, 0.0], [48.0, 48.0]),
        ([16.0, 16.0], [16.0, 16.0]),
        ([0.0, 16.0], [128.0, 16.0]),
        ([-16.0, -16.0], [144.0, 128.0]),
        ([0.0, 0.000001], [128.0, 0.000001]),
    ];
    let mut state = 127u32;
    for _ in 0..120 {
        let mut point = || {
            let mut p = [0.0; 2];
            for v in &mut p {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                *v = (state % 3073) as f32 / 16.0 - 32.0;
            }
            p
        };
        segments.push((point(), point()));
    }
    for repetition in 0..3 {
        for &(a, b) in &segments {
            for (p0, p1) in [(a, b), (b, a)] {
                let batch = count_case(p0, p1)?;
                let expected = references[0].execute_compute(&batch)?;
                assert_eq!(
                    references[1].execute_compute(&batch)?,
                    expected,
                    "wgpu {p0:?} {p1:?}"
                );
                let d = dx12.submit_compute(&batch).map_err(|e| format!("{e:?}"))?;
                let v = vulkan
                    .submit_compute(&batch)
                    .map_err(|e| format!("{e:?}"))?;
                assert_eq!(
                    d.readback()?,
                    expected,
                    "DX12 {p0:?} {p1:?} repeat {repetition}"
                );
                assert_eq!(
                    v.readback()?,
                    expected,
                    "Vulkan {p0:?} {p1:?} repeat {repetition}"
                );
                let counts: Vec<u32> = expected[1]
                    .chunks_exact(4)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                    .collect();
                let cpu = line_scanned_tile_count(
                    Line {
                        path_id: 0,
                        _pad: 0.0,
                        p0,
                        p1,
                    },
                    TileBbox {
                        x0: 0,
                        y0: 0,
                        x1: 8,
                        y1: 8,
                    },
                    (8, 8),
                );
                assert_eq!(
                    counts[7..71].iter().sum::<u32>(),
                    cpu,
                    "CPU count {p0:?} {p1:?}"
                );
                for bytes in &expected {
                    for chunk in bytes[..28]
                        .chunks_exact(4)
                        .chain(bytes[284..].chunks_exact(4))
                    {
                        assert_eq!(u32::from_le_bytes(chunk.try_into().unwrap()), 0x12345678);
                    }
                }
            }
        }
    }
    dx12.assert_valid()?;
    vulkan.assert_valid()?;
    Ok(())
}
