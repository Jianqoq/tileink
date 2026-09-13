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
    let abi: serde_json::Value =
        serde_json::from_str(include_str!("../src/shaders/probe-abi.json")).unwrap();
    spirv::validate(artifact.bytes, "copy_words", &abi).unwrap();
    assert!(spirv::validate(artifact.bytes, "wrong_entry", &abi).is_err());
    let mut wrong = abi.clone();
    wrong["parameter_offsets"]["value"] = serde_json::json!(20);
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
    wrong["workgroup"] = serde_json::json!([32, 1, 1]);
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
    let abi: serde_json::Value =
        serde_json::from_str(include_str!("../src/shaders/probe-abi.json")).unwrap();
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
    wrong["bindings"]["texels"] = serde_json::json!(4);
    assert!(spirv::validate(artifact.bytes, "sample_words", &wrong).is_err());
}
