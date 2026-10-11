#[allow(dead_code)]
#[path = "../build/native/abi.rs"]
mod abi;
#[allow(dead_code)]
#[path = "../build/gpu_constants.rs"]
mod gpu_constants;
#[path = "../build/native/interfaces.rs"]
mod interfaces;
#[allow(dead_code)]
#[path = "../build/native/program.rs"]
mod program;
#[allow(dead_code)]
#[path = "../build/native/source.rs"]
mod source;

#[test]
fn every_catalog_entry_has_a_valid_unique_interface_and_ordered_bindings() {
    let mut families = std::collections::BTreeSet::new();
    let mut entries = std::collections::BTreeSet::new();
    for &(family, _) in program::FAMILIES {
        assert!(families.insert(family), "duplicate family {family}");
        let interface = interfaces::get(family).unwrap();
        abi::validate(&interface).unwrap();
        for entry in interface.entries.keys() {
            assert!(entries.insert(entry.clone()), "duplicate entry {entry}");
            let declarations = abi::binding_declarations(&interface, entry).unwrap();
            let mut resources = interface.resources_for(entry).unwrap();
            resources.sort_by_key(|(_, resource)| resource.binding);
            let mut remaining = declarations.as_str();
            for (_, resource) in resources {
                let slot = format!("slot: {},", resource.binding);
                let index = remaining.find(&slot).unwrap();
                remaining = &remaining[index + slot.len()..];
            }
        }
    }
    assert!(interfaces::get("").is_err());
    assert!(interfaces::get("unknown").is_err());
}

#[test]
fn production_texture_tables_share_the_shader_owned_capacity_and_binding() {
    let capacity = gpu_constants::read_hlsl("shared/texture_table_constants.hlsli").unwrap()["NATIVE_TEXTURE_TABLE_CAPACITY"];
    for (family, name) in [
        ("fine-main", "images"),
        ("fine-brush", "image_resource_textures"),
        ("filter-brush", "image_resource_textures"),
        ("filter-progressive", "texture_table"),
        ("texture-table-validation", "texture_table"),
    ] {
        let interface = interfaces::get(family).unwrap();
        let table = &interface.resources[name];
        assert_eq!(table.kind, abi::Kind::TextureTable, "{family}");
        assert_eq!(table.binding, 30, "{family}");
        assert_eq!(table.count, capacity, "{family}");
        assert_eq!(table.size, 4, "{family}");
        assert!(table.fields.is_empty());
        assert!(!table.internal);
        assert!(
            interface
                .entries
                .values()
                .any(|names| names.iter().any(|n| n == name))
        );
    }
}

#[test]
fn catalog_buffer_permissions_match_declared_hlsl_resources() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for &(family, source) in program::FAMILIES {
        let prepared = program::prepare(root, family, source).unwrap();
        for (name, resource) in &prepared.description.resources {
            let (ty, register) = match resource.kind {
                abi::Kind::Read => ("ByteAddressBuffer", 't'),
                abi::Kind::Write => ("RWByteAddressBuffer", 'u'),
                _ => continue,
            };
            let expected = format!("{ty}{name}:register({register}{});", resource.binding);
            assert!(
                prepared.graph.expanded.lines().any(|line| line
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
                    .replace(",space0", "")
                    == expected),
                "{family}/{name}: expected {expected}"
            );
        }
    }
}
