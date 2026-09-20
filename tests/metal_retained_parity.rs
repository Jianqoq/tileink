//! Exact same-device M5 acceptance: shared 29-state corpus, independent renderers.
#![cfg(all(target_os = "macos", any(feature = "wgpu", feature = "metal")))]
#![allow(dead_code)]
#[path = "../examples/common/mod.rs"]
mod common;
#[cfg(feature = "wgpu")]
#[path = "../examples/common/benchmark_gpu.rs"]
mod gpu;
#[cfg(feature = "wgpu")]
#[path = "../examples/wgpu_backend_parity/readback.rs"]
mod readback;
#[path = "../examples/wgpu_backend_parity/retained_contract.rs"]
mod retained_contract;
#[cfg(feature = "metal")]
#[path = "../examples/wgpu_backend_parity/native/retained.rs"]
mod retained_native;
#[path = "../examples/wgpu_backend_parity/retained_sequence.rs"]
mod retained_sequence;
#[cfg(feature = "wgpu")]
#[path = "../examples/wgpu_backend_parity/retained_wgpu.rs"]
mod retained_wgpu;
use retained_contract::Target;
use retained_sequence::{FRAMES, Sequence};
#[cfg(feature = "metal")]
use serde_json::Value;
use serde_json::json;
use sha2::{Digest, Sha256};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect()
}

#[test]
#[ignore = "fresh same-device retained references: run_native_metal_tests.sh --retained"]
fn complete_retained_sequence() -> Result {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/metal-validation/retained");
    std::fs::create_dir_all(&directory)?;
    let fonts = common::fonts::Snapshot::system()?;
    let input = json!({"fonts":fonts.manifest,"sequence_sha256":hash(include_bytes!("../examples/wgpu_backend_parity/retained_sequence/state.rs")),"frames":retained_sequence::names()});
    let mut sequence = Sequence::new(&fonts)?;
    let mut variants = Vec::new();
    #[cfg(feature = "wgpu")]
    {
        std::fs::write(
            directory.join("input.json"),
            serde_json::to_vec_pretty(&input)?,
        )?;
        let mut identity = None;
        for portable in [false, true] {
            let (metadata, device, queue) =
                gpu::device("metal", portable, false, wgpu::MemoryHints::MemoryUsage);
            if let Some(ref expected) = identity {
                assert_eq!(&metadata["physical_identity"], expected);
            }
            identity = Some(metadata["physical_identity"].clone());
            std::fs::write(
                directory.join("identity.json"),
                serde_json::to_vec_pretty(&metadata)?,
            )?;
            for kind in [Target::Owned, Target::Transient, Target::Persistent] {
                for full in [false, true] {
                    variants.push(retained_wgpu::Variant::new(
                        if portable {
                            "wgpu-portable"
                        } else {
                            "wgpu-native"
                        },
                        &device,
                        &queue,
                        &fonts,
                        kind,
                        full,
                    ));
                }
            }
        }
    }
    #[cfg(feature = "metal")]
    let context = {
        assert_eq!(
            input,
            serde_json::from_slice::<Value>(&std::fs::read(directory.join("input.json"))?)?,
            "retained inputs changed; regenerate reference"
        );
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(directory.join("identity.json"))?)?;
        let context = tileink::NativeContext::new(
            tileink::NativeBackend::Metal,
            &tileink::NativeContextOptions {
                physical_adapter: Some(
                    metadata["physical_identity"]
                        .as_str()
                        .ok_or("missing GPU identity")?
                        .into(),
                ),
                validation: true,
            },
        )?;
        for kind in [Target::Owned, Target::Transient, Target::Persistent] {
            for full in [false, true] {
                variants.push(retained_native::Variant::new(
                    &context,
                    "native-metal",
                    metadata.clone(),
                    &fonts,
                    kind,
                    full,
                )?);
            }
        }
        context
    };
    let mut rows = Vec::new();
    for (index, &frame) in FRAMES.iter().enumerate() {
        sequence.apply(frame)?;
        let file = directory.join(format!("{index:02}-{}.bin", frame.name()));
        #[cfg(feature = "wgpu")]
        let mut reference: Option<Vec<u8>> = None;
        #[cfg(feature = "metal")]
        let reference = std::fs::read(&file)?;
        for variant in &mut variants {
            let (image, mut row) = variant.render(&sequence, frame)?;
            #[cfg(feature = "wgpu")]
            variant.validate(&sequence, frame, &image)?;
            let bytes: &[u8] = bytemuck::cast_slice(&image.pixels);
            #[cfg(feature = "wgpu")]
            let expected = reference.get_or_insert_with(|| bytes.to_vec());
            #[cfg(feature = "metal")]
            let expected = &reference;
            if bytes != expected.as_slice() {
                std::fs::write(
                    directory.join(format!("{index:02}-{}.failed.bin", variant.name)),
                    bytes,
                )?;
                return Err(format!(
                    "{} {}: {} different bytes (length {} vs {})",
                    variant.name,
                    frame.name(),
                    bytes
                        .iter()
                        .zip(expected.iter())
                        .filter(|(a, b)| a != b)
                        .count(),
                    bytes.len(),
                    expected.len()
                )
                .into());
            }
            row["width"] = json!(image.width);
            row["height"] = json!(image.height);
            row["sha256"] = json!(hash(bytes));
            row["different_bytes"] = json!(0);
            rows.push(row);
        }
        #[cfg(feature = "wgpu")]
        std::fs::write(file, reference.ok_or("no reference variants")?)?;
    }
    #[cfg(feature = "metal")]
    context.check_validation()?;
    let backend = if cfg!(feature = "metal") {
        "native"
    } else {
        "wgpu"
    };
    std::fs::write(
        directory.join(format!("{backend}-results.json")),
        serde_json::to_vec_pretty(
            &json!({"frames":FRAMES.len(),"variants":variants.len(),"results":rows,"different_bytes":0}),
        )?,
    )?;
    Ok(())
}
