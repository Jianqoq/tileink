use crate::{Canvas, shared::gpu_plan::CoarseBinningStats};

use super::{
    COUNT_STORAGE_BINDING_COUNT, EMIT_STORAGE_BINDING_COUNT, GpuBufferLengths,
    PREFIX_STORAGE_BINDING_COUNT, coarse_binning_costs, count_layout_entries, emit_layout_entries,
    prefer_dense_binning, prefix_layout_entries, profile_coarse_passes_value,
};

#[test]
fn coarse_pipeline_storage_bindings_match_split_layouts() {
    assert_eq!(
        storage_count(&count_layout_entries()),
        COUNT_STORAGE_BINDING_COUNT
    );
    assert_eq!(
        storage_count(&prefix_layout_entries()),
        PREFIX_STORAGE_BINDING_COUNT
    );
    assert_eq!(
        storage_count(&emit_layout_entries()),
        EMIT_STORAGE_BINDING_COUNT
    );
    assert_eq!(COUNT_STORAGE_BINDING_COUNT, 9);
    assert_eq!(PREFIX_STORAGE_BINDING_COUNT, 10);
    assert_eq!(EMIT_STORAGE_BINDING_COUNT, 9);
}

#[test]
fn coarse_layout_entries_are_contiguous() {
    for entries in [
        count_layout_entries(),
        prefix_layout_entries(),
        emit_layout_entries(),
    ] {
        assert_contiguous_bindings(&entries);
    }
}

#[test]
fn profile_coarse_passes_only_accepts_one() {
    assert!(profile_coarse_passes_value(Some("1")));
    assert!(!profile_coarse_passes_value(None));
    assert!(!profile_coarse_passes_value(Some("true")));
    assert!(!profile_coarse_passes_value(Some("0")));
}

#[test]
fn dense_binning_replaces_many_mostly_idle_incremental_workgroups() {
    let canvas = Canvas::new(3200, 2000, 1.0);
    let lengths = GpuBufferLengths::from_scene(&canvas);
    let stats =
        |active_tiles, compact_candidate_rounds, dense_candidate_rounds| CoarseBinningStats {
            active_tiles,
            compact_candidate_rounds,
            dense_candidate_rounds,
        };

    assert!(!prefer_dense_binning(lengths, stats(128, 128, 104)));
    assert!(prefer_dense_binning(lengths, stats(4096, 4096, 104)));
    assert!(!prefer_dense_binning(
        lengths,
        stats(4096, 4096 * 2, 104 * 512)
    ));
    assert!(!prefer_dense_binning(lengths, stats(0, 0, 104)));

    let (compact, dense) = coarse_binning_costs(lengths, stats(4096, 4096, 104));
    assert!(dense < compact);
}

fn assert_contiguous_bindings(entries: &[::wgpu::BindGroupLayoutEntry]) {
    for (expected, entry) in entries.iter().enumerate() {
        assert_eq!(entry.binding, expected as u32);
    }
}

fn storage_count(entries: &[::wgpu::BindGroupLayoutEntry]) -> u32 {
    entries.iter().filter(|entry| is_storage(entry)).count() as u32
}

fn is_storage(entry: &::wgpu::BindGroupLayoutEntry) -> bool {
    matches!(
        entry.ty,
        ::wgpu::BindingType::Buffer {
            ty: ::wgpu::BufferBindingType::Storage { .. },
            ..
        }
    )
}

// Seed the actual coarse buffers directly: this checks allocation semantics without a large
// canvas or depending on count/fine kernels to manufacture the expected ranges.
#[test]
fn wgpu_coarse_prefix_preserves_ranges_across_chunk_boundaries() {
    if std::env::var("TILEINK_RUN_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }
    use crate::wgpu::buffer::WgpuBuffer;
    use bytemuck::Zeroable;

    let renderer = crate::Renderer::new_default_device(1, 1, peniko::Color::TRANSPARENT);
    let (device, queue) = (renderer.device(), renderer.queue());
    let pipeline = super::WgpuCoarsePipeline::new(device, None, &Default::default())
        .expect("coarse prefix test requires storage buffer support");
    let mut work = WgpuBuffer::new(device, "prefix test work");
    let mut chunks = WgpuBuffer::new(device, "prefix test chunks");
    let mut unused_read = WgpuBuffer::new(device, "prefix test unused read bindings");
    unused_read.upload(
        device,
        queue,
        "prefix test unused read bindings",
        &[0u32; 64],
    );
    let mut unused_write = WgpuBuffer::new(device, "prefix test unused write binding");
    unused_write.upload(
        device,
        queue,
        "prefix test unused write binding",
        &[0u32; 64],
    );

    // The final cases cross one and two 256-chunk carry boundaries, including a partial block.
    for count in [0u32, 1, 255, 256, 257, 65_535, 65_536, 65_537, 131_073] {
        for sparse in [false, true] {
            let tiles = if sparse { count * 2 + 7 } else { count };
            let chunk_count = count.div_ceil(256);
            let mut words = vec![u32::MAX; tiles.max(1) as usize * 6];
            for tile in 0..tiles as usize {
                words[tile * 6] = if tile % 7 < 3 { 0 } else { (tile % 17) as u32 };
                words[tile * 6 + 3] = ((tile * 7) % 19) as u32;
            }
            // Reverse and separate the active tiles so a dense-index substitution is detected.
            let active: Vec<u32> = (0..count)
                .map(|item| {
                    if sparse {
                        2 * (count - 1 - item) + 1
                    } else {
                        item
                    }
                })
                .collect();
            let active_base = words.len() as u32;
            if sparse {
                words.extend_from_slice(&active);
            }
            let mut expected = words.clone();
            let mut expected_chunks = vec![[u32::MAX; 4]; chunk_count as usize + 1];
            let mut totals = [0u32; 2];
            for (index, block) in active.chunks(256).enumerate() {
                let before = totals;
                for &tile in block {
                    let base = tile as usize * 6;
                    for (channel, total) in totals.iter_mut().enumerate() {
                        let offset = base + channel * 3;
                        expected[offset + 1] = *total;
                        *total += expected[offset];
                        expected[offset + 2] = *total;
                    }
                }
                expected_chunks[index] = [
                    totals[0] - before[0],
                    before[0],
                    totals[1] - before[1],
                    before[1],
                ];
            }
            work.upload(device, queue, "prefix test work", &words);
            chunks.upload(
                device,
                queue,
                "prefix test chunks",
                &vec![[u32::MAX; 4]; expected_chunks.len()],
            );
            queue.write_buffer(
                &pipeline.config,
                0,
                bytemuck::bytes_of(&super::CoarseConfig {
                    tile_count: tiles,
                    chunk_count,
                    active_tile_count: count,
                    active_tile_list_base: active_base,
                    incremental: u32::from(sparse),
                    ..super::CoarseConfig::zeroed()
                }),
            );
            let entries: Vec<_> = (0..=10)
                .map(|binding| {
                    if binding == 0 {
                        super::bind_config_buffer(0, &pipeline.config, 0, pipeline.config_size)
                    } else {
                        super::bind_buffer(
                            binding,
                            match binding {
                                4 => unused_write.buffer(),
                                7 => work.buffer(),
                                8 => chunks.buffer(),
                                _ => unused_read.buffer(),
                            },
                        )
                    }
                })
                .collect();
            let bindings = device.create_bind_group(&::wgpu::BindGroupDescriptor {
                label: Some("coarse prefix semantics"),
                layout: &pipeline.prefix_bind_group_layout,
                entries: &entries,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_bind_group(0, &bindings, &[]);
                for (kernel, groups) in [
                    (&pipeline.prefix_chunks, chunk_count),
                    (&pipeline.chunk_offsets, 1),
                    (&pipeline.apply_chunk_offsets, chunk_count),
                ] {
                    pass.set_pipeline(pipeline.pipeline(device, kernel));
                    pass.dispatch_workgroups(groups, 1, 1);
                }
            }
            queue.submit([encoder.finish()]);
            for (index, (actual, expected)) in work
                .read::<u32>(device, queue, expected.len())
                .into_iter()
                .zip(expected)
                .enumerate()
            {
                assert_eq!(
                    actual, expected,
                    "count={count}, sparse={sparse}, work word={index}"
                );
            }
            assert_eq!(
                chunks.read::<[u32; 4]>(device, queue, expected_chunks.len()),
                expected_chunks,
                "count={count}, sparse={sparse}, chunk totals/offsets"
            );
        }
    }
}

#[test]
fn dense_binning_accounts_for_the_shared_prefix_chain() {
    // Sharing offsets makes dense binning cheaper at this boundary; the former duplicated
    // prefix cost incorrectly keeps these moderately sparse tiles on the compact path.
    let lengths = GpuBufferLengths::from_scene(&Canvas::new(3200, 2000, 1.0));
    assert!(prefer_dense_binning(
        lengths,
        CoarseBinningStats {
            active_tiles: 160,
            compact_candidate_rounds: 160,
            dense_candidate_rounds: 104,
        }
    ));
}
