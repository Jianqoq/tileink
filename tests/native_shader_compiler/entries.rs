//! Compile the actual entry catalog even on Metal-only test hosts.
use super::{dxc, pinned_compiler, source};

#[allow(dead_code)]
#[path = "../../build/native/abi.rs"]
mod abi;
#[allow(dead_code)]
#[path = "../../build/native/dxil_reflection.rs"]
mod dxil_reflection;
#[path = "../../build/native/interfaces.rs"]
mod interfaces;
#[allow(dead_code)]
#[path = "../../build/native/program.rs"]
mod program;

#[test]
#[ignore = "requires explicitly pinned TILEINK_NATIVE_DXC_PATH"]
fn every_hlsl_entry_compiles_for_both_targets() {
    let compiler = pinned_compiler();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = tempfile::tempdir().unwrap();
    let mut count = 0;
    for &(family, source) in program::FAMILIES {
        let prepared = program::prepare(root, family, source).unwrap();
        for entry in prepared.description.entries.keys() {
            for target in ["dxil", "spirv"] {
                let directory = work.path().join(format!("{entry}-{target}"));
                let bytes = compiler
                    .compile(
                        &prepared.graph.expanded,
                        &dxc::Dxc::flags(target, entry).unwrap(),
                        &directory,
                        target,
                    )
                    .unwrap_or_else(|error| panic!("{entry} ({target}): {error}"));
                if target == "dxil" {
                    dxil_reflection::validate(
                        &compiler.reflect_dxil(&bytes, &directory).unwrap(),
                        entry,
                        &prepared.description,
                    )
                    .unwrap_or_else(|error| panic!("{entry} ({target}) ABI: {error}"));
                }
                count += 1;
            }
        }
    }
    eprintln!("compiled {count} HLSL entry/target pairs; DXIL reflection matches the host ABI");
}
