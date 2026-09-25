use super::*;

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn scan_prefix_preserves_guards_empty_chunks_and_wrapping_ranges() -> Result<()> {
    let mut device = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for lengths in [
        vec![0],
        vec![1],
        vec![255],
        vec![256],
        vec![256, 1],
        vec![256, 0, 17, 1, 255],
    ] {
        for seed in [0, 19, u32::MAX] {
            let (batch, expected) = scan_cases::prefix_case(&lengths, seed)?;
            let ticket = device.submit_compute(&batch)?;
            assert_eq!(device.readback_batch(&ticket)?, expected);
        }
    }
    device.assert_valid()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn scan_count_matches_cpu_clipped_tile_counts_and_keeps_guards() -> Result<()> {
    use crate::shared::{bounds::TileBbox, scan_line::line_scanned_tile_count};
    let mut device = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
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
    let mut seed = 127u32;
    for _ in 0..120 {
        let mut point = || {
            std::array::from_fn(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed % 3073) as f32 / 16.0 - 32.0
            })
        };
        segments.push((point(), point()));
    }
    for (a, b) in segments {
        for (a, b) in [(a, b), (b, a)] {
            let batch = scan_cases::count_case(a, b)?;
            let ticket = device.submit_compute(&batch)?;
            let outputs = device.readback_batch(&ticket)?;
            for output in &outputs {
                let words: Vec<u32> = output
                    .chunks_exact(4)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                    .collect();
                assert!(
                    words[..7]
                        .iter()
                        .chain(&words[71..])
                        .all(|&word| word == 0x12345678)
                );
            }
            let total: u32 = outputs[1][28..284]
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .sum();
            assert_eq!(
                total,
                line_scanned_tile_count(
                    crate::shared::line::Line {
                        path_id: 0,
                        _pad: 0.0,
                        p0: a,
                        p1: b
                    },
                    TileBbox {
                        x0: 0,
                        y0: 0,
                        x1: 8,
                        y1: 8
                    },
                    (8, 8),
                ),
                "{a:?} -> {b:?}"
            );
        }
    }
    device.assert_valid()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn scan_invalid_path_references_leave_count_and_emit_outputs_untouched() -> Result<()> {
    let mut device = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for entry in ["scan_count", "scan_emit"] {
        let mut batch = ComputeBatch::new();
        let config =
            batch.buffer(bytemuck::cast_slice(&[0u32, 0, 1, 0, 3, 8, 0, 0, 0, 0, 0]).to_vec())?;
        let lines: Vec<u32> = [1, u32::MAX / 19, u32::MAX]
            .into_iter()
            .flat_map(|path| [path, 0, 0, 0, 16f32.to_bits(), 16f32.to_bits()])
            .collect();
        let lines = batch.buffer(bytemuck::cast_slice(&lines).to_vec())?;
        let paths = batch.buffer(vec![0; 19 * 4])?;
        let guard = vec![0xa5; 128];
        let first = batch.buffer(guard.clone())?;
        let second = batch.buffer(guard.clone())?;
        let indices = batch.buffer(vec![0; 4])?;
        // SAFETY: the shared scan contract explicitly treats absent path records
        // as empty geometry. All line records and output allocations are valid.
        unsafe {
            batch.dispatch(
                entry,
                &[
                    (0, config),
                    (1, lines),
                    (2, paths),
                    (3, first),
                    (4, second),
                    (5, indices),
                ],
                [1, 1, 1],
            )?;
        }
        batch.readback(first)?;
        batch.readback(second)?;
        let ticket = device.submit_compute(&batch)?;
        assert_eq!(
            device.readback_batch(&ticket)?,
            vec![guard.clone(), guard],
            "{entry}"
        );
    }
    device.assert_valid()
}
