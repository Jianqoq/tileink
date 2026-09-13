#[allow(dead_code)]
#[path = "../build/native/abi.rs"]
mod abi;
#[allow(dead_code)]
#[path = "../build/gpu_constants.rs"]
mod gpu_constants;
#[allow(dead_code)]
#[path = "../build/native/interfaces.rs"]
mod interfaces;
#[cfg(all(
    feature = "native-vulkan",
    any(target_os = "windows", target_os = "linux")
))]
#[path = "../build/native/spirv.rs"]
mod spirv;

#[cfg(all(
    feature = "native-vulkan",
    any(target_os = "windows", target_os = "linux")
))]
#[test]
fn actual_spirv_rejects_wrong_entry_layout_binding_stride_and_workgroup() {
    let artifact = tileink::NATIVE_SHADER_ARTIFACTS
        .iter()
        .find(|a| a.format == "spirv" && a.entry == "copy_words")
        .unwrap();
    let abi = interfaces::get("probe").unwrap();
    spirv::validate(artifact.bytes, "copy_words", &abi).unwrap();
    assert!(spirv::validate(artifact.bytes, "wrong_entry", &abi).is_err());
    let mut wrong = abi.clone();
    wrong.resources.get_mut("params").unwrap().fields[4].offset = 20;
    assert!(spirv::validate(artifact.bytes, "copy_words", &wrong).is_err());
    for decoration in [6, 33] {
        let mut words: Vec<u32> = artifact
            .bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let mut index = 5;
        while index < words.len() {
            if words[index] & 65535 == 71 && words[index + 2] == decoration {
                words[index + 3] += 4;
                break;
            }
            index += (words[index] >> 16) as usize;
        }
        assert!(index < words.len());
        let bytes: Vec<_> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert!(spirv::validate(&bytes, "copy_words", &abi).is_err());
    }
    let mut wrong = abi.clone();
    wrong.workgroup = [32, 1, 1];
    assert!(spirv::validate(artifact.bytes, "copy_words", &wrong).is_err());
    for length in [0, 4, 20, artifact.bytes.len() - 1] {
        assert!(spirv::validate(&artifact.bytes[..length], "copy_words", &abi).is_err());
    }
}

#[cfg(all(
    feature = "native-vulkan",
    any(target_os = "windows", target_os = "linux")
))]
#[test]
fn actual_spirv_texture_dimension_array_sample_type_and_binding_are_checked() {
    let artifact = tileink::NATIVE_SHADER_ARTIFACTS
        .iter()
        .find(|a| a.format == "spirv" && a.entry == "sample_words")
        .unwrap();
    let abi = interfaces::get("probe").unwrap();
    spirv::validate(artifact.bytes, "sample_words", &abi).unwrap();
    for (operand, wrong_value) in [(3, 2), (5, 1), (6, 1), (7, 2), (8, 4)] {
        let mut words: Vec<_> = artifact
            .bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let mut index = 5;
        while index < words.len() && words[index] & 65535 != 25 {
            index += (words[index] >> 16) as usize;
        }
        assert!(index < words.len());
        words[index + operand] = wrong_value;
        let bytes: Vec<_> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert!(spirv::validate(&bytes, "sample_words", &abi).is_err());
    }
    let mut wrong = abi.clone();
    wrong.resources.get_mut("texels").unwrap().binding = 4;
    assert!(spirv::validate(artifact.bytes, "sample_words", &wrong).is_err());
}

#[cfg(all(
    feature = "native-vulkan",
    any(target_os = "windows", target_os = "linux")
))]
#[test]
fn actual_range_scatter_spirv_uses_the_production_dispatch_shape() {
    let artifact = tileink::NATIVE_SHADER_ARTIFACTS
        .iter()
        .find(|a| a.format == "spirv" && a.entry == "range_scatter")
        .unwrap();
    let abi = interfaces::get("range-scatter").unwrap();
    spirv::validate(artifact.bytes, "range_scatter", &abi).unwrap();
    assert_eq!(artifact.workgroup, [256, 1, 1]);
    let mut wrong = abi.clone();
    wrong.workgroup = [64, 1, 1];
    assert!(spirv::validate(artifact.bytes, "range_scatter", &wrong).is_err());
    let mut wrong = abi.clone();
    wrong.resources.get_mut("source").unwrap().binding = 0;
    assert!(spirv::validate(artifact.bytes, "range_scatter", &wrong).is_err());
}

#[cfg(all(
    feature = "native-vulkan",
    any(target_os = "windows", target_os = "linux")
))]
#[test]
fn actual_cumsum_spirv_checks_each_uniform_block_and_shared_workgroup() {
    let abi = interfaces::get("cumsum").unwrap();
    for artifact in tileink::NATIVE_SHADER_ARTIFACTS
        .iter()
        .filter(|a| a.format == "spirv" && a.entry.starts_with("cumsum_"))
    {
        spirv::validate(artifact.bytes, artifact.entry, &abi).unwrap();
        for path in ["config", "dispatch_grid"] {
            if !abi.entries[artifact.entry].iter().any(|v| v == path) {
                continue;
            }
            for field in ["offset", "name"] {
                let mut wrong = abi.clone();
                let member = &mut wrong.resources.get_mut(path).unwrap().fields[0];
                if field == "offset" {
                    member.offset = 4;
                } else {
                    member.name = "different_field".into();
                }
                assert!(
                    spirv::validate(artifact.bytes, artifact.entry, &wrong).is_err(),
                    "{} {path} {field}",
                    artifact.entry
                );
            }
            let mut wrong = abi.clone();
            wrong.resources.get_mut(path).unwrap().binding = 30;
            assert!(spirv::validate(artifact.bytes, artifact.entry, &wrong).is_err());
        }
        let mut wrong = abi.clone();
        wrong.workgroup = [32, 1, 1];
        assert!(spirv::validate(artifact.bytes, artifact.entry, &wrong).is_err());
    }
}
