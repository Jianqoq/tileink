#[cfg(any(feature = "native-dx12", feature = "native-vulkan"))]
#[test]
fn native_build_embeds_each_expected_nonempty_program() {
    let artifacts = tileink::NATIVE_SHADER_ARTIFACTS;
    let dxil = cfg!(all(feature = "native-dx12", target_os = "windows"));
    let spirv = cfg!(all(
        feature = "native-vulkan",
        any(target_os = "windows", target_os = "linux")
    ));
    let entries = [
        "coarse_count",
        "coarse_count_bins",
        "coarse_emit_chunk_particle_counts",
        "coarse_emit_chunk_particle_offsets",
        "coarse_tile_counts_from_emit_chunks",
        "coarse_emit_chunk_tile_kinds",
        "clear_words",
        "copy_words",
        "layout_words",
        "sample_words",
        "range_scatter",
        "cumsum_prefix_chunks",
        "cumsum_chunk_offsets",
        "cumsum_apply_chunk_offsets",
        "coarse_emit_chunk_counts",
        "coarse_emit_prefix_chunks",
        "coarse_emit_chunk_offsets",
        "coarse_emit_apply_chunk_offsets",
        "coarse_emit_fill_refs",
        "coarse_prefix_chunks",
        "coarse_chunk_offsets",
        "coarse_apply_chunk_offsets",
        "scan_clear",
        "scan_count",
        "scan_emit",
        "scan_prefix_chunks",
        "scan_chunk_offsets",
        "scan_apply_chunk_offsets",
    ];
    assert_eq!(
        artifacts.len(),
        entries.len() * (usize::from(dxil) + usize::from(spirv))
    );
    for format in ["dxil", "spirv"] {
        if (format == "dxil" && !dxil) || (format == "spirv" && !spirv) {
            continue;
        }
        for entry in entries {
            let matches: Vec<_> = artifacts
                .iter()
                .filter(|a| a.format == format && a.entry == entry)
                .collect();
            assert_eq!(matches.len(), 1);
            let artifact = matches[0];
            assert!(!artifact.bytes.is_empty());
            assert_eq!(artifact.cache_key.len(), 64);
            assert!(artifact.cache_key.bytes().all(|b| b.is_ascii_hexdigit()));
            assert_eq!(
                &artifact.bytes[..4],
                if format == "dxil" {
                    b"DXBC"
                } else {
                    &[3, 2, 35, 7]
                }
            );
        }
    }
}
