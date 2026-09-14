use super::super::{
    Result,
    adapter::Adapter,
    compute::ComputeBatch,
    program::scene_scan::{PreparedScan, encode_scene},
};
use crate::render::upload::scene::SceneUploadStaging;
use crate::{Canvas, NativeBackend};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_scene_scan_uses_shared_canvas_geometry() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let native = [
        Adapter::new(NativeBackend::Dx12, &identity)?,
        Adapter::new(NativeBackend::Vulkan, &identity)?,
    ];
    let reference = [
        super::reference::Reference::new(wgpu::Backends::DX12, &identity)?,
        super::reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
    ];
    let wide = crate::shared::gpu_constants::CUMSUM_CHUNK_SIZE
        * crate::shared::gpu_constants::TILE_SIZE
        * 2
        + 1;
    for (width, count) in [(64, 1), (64, 17), (64, 257), (wide, 1)] {
        let mut canvas = Canvas::new(width, 64, 1.0);
        for index in 0..count {
            let delta = (index % 7) as f64 / 8.0;
            let mut path = peniko::kurbo::BezPath::new();
            path.move_to((8.0 + delta, 8.0));
            path.line_to((f64::from(width - 1), 40.0 - delta));
            path.line_to((8.0 + delta, 40.0 - delta));
            canvas.push_path(
                path,
                crate::Brush::Solid(peniko::Color::from_rgb8(255, 0, 0)),
                peniko::kurbo::Affine::IDENTITY,
                crate::FillRule::NonZero,
                0.25,
            );
        }
        assert_eq!(canvas.lines.len(), count as usize * 3);
        let mut staging = SceneUploadStaging::default();
        let prepared = PreparedScan::new(&canvas, &mut staging.path_plans);
        let mut batch = ComputeBatch::new();
        let output = encode_scene(&mut batch, &prepared, 65)?;
        for id in [
            output.paths,
            output.backdrops,
            output.tile_segment_ranges,
            output.segments,
        ] {
            batch.readback(id)?;
        }
        if width == wide {
            assert!(
                batch
                    .passes()
                    .iter()
                    .any(|pass| pass.shader.entry == "cumsum_chunk_offsets")
            );
        }
        let expected = ordered_segments(reference[0].execute_compute(&batch)?);
        assert!(
            expected[1]
                .chunks_exact(4)
                .any(|word| i32::from_le_bytes(word.try_into().unwrap()) != 0),
            "filled triangle must produce nonzero backdrop winding"
        );
        for path in &canvas.path_records {
            let begin = path.data_offset as usize * 8;
            let end = begin + path.data_len as usize * 8;
            for range in expected[2][begin..end].chunks_exact(8) {
                let start = u32::from_le_bytes(range[..4].try_into().unwrap());
                let end = u32::from_le_bytes(range[4..].try_into().unwrap());
                assert!(
                    start >= path.segment_start
                        && end >= start
                        && end <= path.segment_start + path.segment_capacity
                );
            }
        }
        assert_eq!(
            expected[0],
            bytemuck::cast_slice::<_, u8>(&canvas.path_records)
        );
        assert_eq!(
            ordered_segments(reference[1].execute_compute(&batch)?),
            expected,
            "wgpu scene count {count}"
        );
        // Segment atomics may permute contour edges within a tile.
        // Compare each tile's segment multiset; all other buffers remain exact.
        for adapter in &native {
            let actual = adapter
                .submit_compute(&batch)
                .map_err(|e| format!("{e:?}"))?
                .readback()?;
            assert_eq!(
                ordered_segments(actual),
                expected,
                "native scene count {count}"
            );
        }
    }
    for adapter in native {
        adapter.assert_valid()?;
    }
    Ok(())
}

fn ordered_segments(mut outputs: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let stride = std::mem::size_of::<crate::shared::line_seg::LineSegment>();
    let (metadata, storage) = outputs.split_at_mut(3);
    let buffer = &mut storage[0];
    for range in metadata[2].chunks_exact(8) {
        let start = u32::from_le_bytes(range[..4].try_into().unwrap()) as usize * stride;
        let end = u32::from_le_bytes(range[4..].try_into().unwrap()) as usize * stride;
        let mut segments: Vec<_> = buffer[start..end]
            .chunks_exact(stride)
            .map(|v| v.to_vec())
            .collect();
        segments.sort_unstable();
        buffer[start..end].copy_from_slice(&segments.concat());
    }
    outputs
}
