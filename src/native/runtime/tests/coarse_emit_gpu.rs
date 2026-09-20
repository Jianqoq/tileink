use super::reference;
use crate::native::{
    NativeBackend,
    runtime::{Result, adapter::Adapter},
};
#[path = "coarse_allocation_cases.rs"]
mod allocation_cases;
use allocation_cases::{emit_case, particle_offset_case};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_coarse_emit_allocation_preserves_capacity_and_record_guards() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let dx12 = Adapter::new(NativeBackend::Dx12, &identity)?;
    let vulkan = Adapter::new(NativeBackend::Vulkan, &identity)?;
    let references = [
        reference::Reference::new(wgpu::Backends::DX12, &identity)?,
        reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
    ];
    for tiles in [0, 1, 255, 256, 257, 513] {
        for truncate in [false, true] {
            let (batch, expected) = emit_case(tiles, truncate)?;
            for repetition in 0..3 {
                let mut outputs = Vec::new();
                for reference in &references {
                    outputs.push(reference.execute_compute(&batch)?);
                }
                for device in [&dx12, &vulkan] {
                    outputs.push(
                        device
                            .submit_compute(&batch)
                            .map_err(|e| format!("{e:?}"))?
                            .readback()?,
                    );
                }
                for (route, actual) in outputs.iter().enumerate() {
                    assert_eq!(actual.len(), expected.len());
                    for (buffer, (a, b)) in actual.iter().zip(&expected).enumerate() {
                        let first = a.iter().zip(b).position(|(a, b)| a != b);
                        assert!(
                            a.len() == b.len() && first.is_none(),
                            "tiles {tiles} truncate {truncate} repetition {repetition} route {route} buffer {buffer} first {first:?}"
                        );
                    }
                }
            }
        }
    }
    dx12.assert_valid()?;
    vulkan.assert_valid()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_coarse_particle_offsets_match_wrapping_counts_and_tile_ranges() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let dx12 = Adapter::new(NativeBackend::Dx12, &identity)?;
    let vulkan = Adapter::new(NativeBackend::Vulkan, &identity)?;
    let references = [
        reference::Reference::new(wgpu::Backends::DX12, &identity)?,
        reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
    ];
    for tiles in [0, 1, 255, 256, 257, 513] {
        let (batch, expected) = particle_offset_case(tiles)?;
        for repetition in 0..3 {
            for (route, actual) in references
                .iter()
                .map(|r| r.execute_compute(&batch))
                .chain([&dx12, &vulkan].into_iter().map(|r| {
                    r.submit_compute(&batch)
                        .map_err(|e| format!("{e:?}").into())
                        .and_then(|s| s.readback())
                }))
                .enumerate()
            {
                let actual = actual?;
                assert_eq!(actual.len(), expected.len());
                for (buffer, (a, b)) in actual.iter().zip(&expected).enumerate() {
                    let first = a.iter().zip(b).position(|(a, b)| a != b);
                    assert!(
                        a.len() == b.len() && first.is_none(),
                        "tiles {tiles} repetition {repetition} route {route} buffer {buffer} first {first:?}"
                    );
                }
            }
        }
    }
    dx12.assert_valid()?;
    vulkan.assert_valid()?;
    Ok(())
}
