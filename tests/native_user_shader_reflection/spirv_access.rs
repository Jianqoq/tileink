//! Buffer reflection follows DXC v1.8.2502's documented SPIR-V mapping.
//! These modules exercise reflection, independently of native compiler/GPU availability.
use super::{abi, spirv};
use std::collections::BTreeMap;

fn interface(source_kind: abi::Kind) -> abi::Interface {
    let resource = |binding, kind| abi::Resource {
        binding,
        kind,
        size: 4,
        count: 1,
        fields: vec![],
        internal: false,
    };
    abi::Interface {
        workgroup: [1, 1, 1],
        descriptor_set: 0,
        resources: BTreeMap::from([
            ("source".into(), resource(0, source_kind)),
            ("target".into(), resource(1, abi::Kind::Write)),
        ]),
        entries: BTreeMap::from([("main".into(), vec!["source".into(), "target".into()])]),
    }
}
fn instruction(words: &mut Vec<u32>, opcode: u32, operands: &[u32]) {
    words.push(((operands.len() as u32 + 1) << 16) | opcode);
    words.extend_from_slice(operands);
}
fn name(words: &mut Vec<u32>, opcode: u32, prefix: &[u32], text: &str) {
    let mut operands = prefix.to_vec();
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes.resize(bytes.len().next_multiple_of(4), 0);
    operands.extend(
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(u32::from_le_bytes),
    );
    instruction(words, opcode, &operands);
}
fn module(nonwritable: &[(u32, &[u32])]) -> Vec<u8> {
    let mut words = vec![0x07230203, 0x00010000, 0, 32, 0];
    instruction(&mut words, 17, &[1]); // Shader capability.
    instruction(&mut words, 14, &[0, 1]); // Logical GLSL450 memory model.
    name(&mut words, 15, &[5, 20], "main");
    instruction(&mut words, 16, &[20, 17, 1, 1, 1]);
    name(&mut words, 5, &[10], "source");
    name(&mut words, 5, &[11], "target");
    for operands in [
        &[3, 6, 4][..],
        &[4, 3],
        &[5, 3],
        &[10, 34, 0],
        &[10, 33, 0],
        &[11, 34, 0],
        &[11, 33, 1],
    ] {
        instruction(&mut words, 71, operands);
    }
    instruction(&mut words, 72, &[4, 0, 35, 0]);
    instruction(&mut words, 72, &[5, 0, 35, 0]);
    for &(opcode, operands) in nonwritable {
        instruction(&mut words, opcode, operands);
    }
    for (opcode, operands) in [
        (19, &[1][..]),
        (21, &[2, 32, 0]),
        (29, &[3, 2]),
        (30, &[4, 3]),
        (30, &[5, 3]),
        (32, &[6, 2, 4]),
        (32, &[7, 2, 5]),
        (32, &[8, 2, 2]),
        (33, &[9, 1]),
        (43, &[2, 12, 0]),
        (59, &[6, 10, 2]),
        (59, &[7, 11, 2]),
        (54, &[1, 20, 0, 9]),
        (248, &[21]),
        (65, &[8, 22, 10, 12, 12]),
        (61, &[2, 23, 22]),
        (65, &[8, 24, 11, 12, 12]),
        (62, &[24, 23]),
        (253, &[]),
        (56, &[]),
    ] {
        instruction(&mut words, opcode, operands);
    }
    words.into_iter().flat_map(u32::to_le_bytes).collect()
}

#[test]
fn readonly_byte_buffer_rejects_writable_spirv_for_builtin_and_user_interfaces() {
    let bytes = module(&[]);
    let interface = interface(abi::Kind::Read);
    assert!(spirv::validate(&bytes, "main", &interface).is_err());
    assert!(spirv::validate_user(&bytes, "main", &interface).is_err());
}
#[test]
fn readonly_byte_buffer_accepts_member_or_variable_nonwritable() {
    for decoration in [(72, &[4, 0, 24][..]), (71, &[10, 24][..])] {
        let bytes = module(&[decoration]);
        spirv::validate(&bytes, "main", &interface(abi::Kind::Read)).unwrap();
        spirv::validate_user(&bytes, "main", &interface(abi::Kind::Read)).unwrap();
    }
}
#[test]
fn readonly_buffer_requires_nonwritable_on_its_own_variable_or_member() {
    for decoration in [
        (72, &[5, 0, 24][..]),
        (72, &[4, 1, 24][..]),
        (71, &[11, 24][..]),
    ] {
        assert!(
            spirv::validate_user(&module(&[decoration]), "main", &interface(abi::Kind::Read))
                .is_err()
        );
    }
}
#[test]
fn readwrite_buffer_allows_readonly_optimization_or_writable_source() {
    for decorations in [
        vec![],
        vec![(72, &[4, 0, 24][..])],
        vec![(71, &[10, 24][..])],
    ] {
        let bytes = module(&decorations);
        spirv::validate(&bytes, "main", &interface(abi::Kind::Write)).unwrap();
        spirv::validate_user(&bytes, "main", &interface(abi::Kind::Write)).unwrap();
    }
}
