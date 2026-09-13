#[cfg(any(feature = "native-dx12", feature = "native-vulkan"))]
#[test]
fn native_build_embeds_each_expected_nonempty_probe() {
    let artifacts = tileink::NATIVE_SHADER_ARTIFACTS;
    let dxil = cfg!(all(feature = "native-dx12", target_os = "windows"));
    let spirv = cfg!(all(
        feature = "native-vulkan",
        any(target_os = "windows", target_os = "linux")
    ));
    assert_eq!(
        artifacts.len(),
        4 * (usize::from(dxil) + usize::from(spirv))
    );
    for format in ["dxil", "spirv"] {
        if (format == "dxil" && !dxil) || (format == "spirv" && !spirv) {
            continue;
        }
        for entry in ["clear_words", "copy_words", "layout_words", "sample_words"] {
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
