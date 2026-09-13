use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[test]
fn migration_inventory_covers_every_reference_entry_and_texture_variant() {
    let reference_bytes = include_bytes!("../docs/native/wgpu-reference-inventory.json");
    let reference: serde_json::Value = serde_json::from_slice(reference_bytes).unwrap();
    let inventory: serde_json::Value =
        serde_json::from_str(include_str!("../docs/native/native-program-inventory.json")).unwrap();
    assert_eq!(
        inventory["reference_sha256"],
        reference_digest(reference_bytes)
    );
    let key = |source: &serde_json::Value,
               table: &serde_json::Value,
               program: &serde_json::Value| {
        serde_json::to_string(&(source, table, &program["entry"], &program["workgroup"])).unwrap()
    };
    let mut expected = BTreeSet::new();
    for module in reference["modules"].as_array().unwrap() {
        for program in module["programs"].as_array().unwrap() {
            assert!(expected.insert(key(&module["source"], &module["texture_table"], program)));
        }
    }
    let mut actual = BTreeSet::new();
    for program in inventory["programs"].as_array().unwrap() {
        assert!(actual.insert(key(&program["source"], &program["texture_table"], program)));
        assert_eq!(program["hlsl"], "unported");
        assert_eq!(program["msl"], "unported");
    }
    assert_eq!(actual, expected);
}

fn reference_digest(bytes: &[u8]) -> String {
    // Git may check out JSON as CRLF on Windows. Normalize only line endings;
    // content edits must still invalidate the migration inventory reference.
    let source = std::str::from_utf8(bytes).unwrap().replace("\r\n", "\n");
    Sha256::digest(source.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn reference_digest_survives_git_checkout_line_endings() {
    assert_eq!(reference_digest(b"{}\n"), reference_digest(b"{}\r\n"));
}
