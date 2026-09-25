use super::*;
use crate::shared::{
    filter_config::FilterConfig,
    gpu_constants::{SDF_PROBE_REQUEST_WORDS, SDF_RECORD_WORDS},
};
use sha2::{Digest, Sha256};
#[path = "../../../../tests/shaders/sdf_cases.rs"]
mod sdf_cases;

#[test]
#[ignore = "run scripts/mac/run_native_metal_tests.sh to generate fresh same-GPU WGSL evidence"]
fn sdf_all_shapes_and_transforms_match_same_device_wgsl_exactly() -> Result<()> {
    let (records, requests, oracle) = sdf_cases::cases();
    let count = requests.len() / SDF_PROBE_REQUEST_WORDS as usize;
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/metal-validation/math");
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("sdf.json"))?)?;
    let expected = std::fs::read(directory.join("sdf.bin"))?;
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        metadata["records_sha256"],
        digest(bytemuck::cast_slice(&records))
    );
    assert_eq!(
        metadata["requests_sha256"],
        digest(bytemuck::cast_slice(&requests))
    );
    assert_eq!(metadata["output_sha256"], digest(&expected));
    let identity = metadata["identity"]["physical_identity"]
        .as_str()
        .ok_or("missing physical registry ID")?;
    let mut device = Metal::with_options(&crate::NativeContextOptions {
        physical_adapter: Some(identity.into()),
        validation: true,
    })?;
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(
        bytemuck::bytes_of(&FilterConfig {
            pixel_count: count as u32,
            ..Default::default()
        })
        .to_vec(),
    )?;
    let paint = batch.buffer(bytemuck::cast_slice(&records).to_vec())?;
    let positions = batch.buffer(bytemuck::cast_slice(&requests).to_vec())?;
    let output = batch.buffer(bytemuck::cast_slice(&vec![0xa1b2c3d4u32; count + 4]).to_vec())?;
    // SAFETY: all requests address complete records; the extra workgroup and four
    // trailing output words verify the logical count guard on the production entry.
    unsafe {
        batch.dispatch(
            "sdf_coverage_words",
            &[(0, config), (5, positions), (6, output), (7, paint)],
            [(count as u32).div_ceil(256) + 1, 1, 1],
        )?;
    }
    batch.readback(output)?;
    for repetition in 0..3 {
        let ticket = device.submit_compute(&batch)?;
        let actual = device.readback_batch(&ticket)?.remove(0);
        assert_eq!(
            &actual[..oracle.len() * 4],
            bytemuck::cast_slice::<u32, u8>(&oracle)
        );
        let mut differences = std::collections::BTreeMap::<u32, usize>::new();
        for (request, (a, b)) in actual
            .chunks_exact(4)
            .zip(expected.chunks_exact(4))
            .enumerate()
            .take(count)
        {
            if a != b {
                *differences
                    .entry(records[requests[request * 12] as usize])
                    .or_default() += 1;
            }
        }
        if !differences.is_empty() {
            std::fs::write(directory.join("sdf-native.bin"), &actual)?;
            eprintln!("SDF differing samples by shape kind: {differences:?}");
        }
        let first = actual.iter().zip(&expected).position(|(a, b)| a != b);
        if let Some(index) = first {
            let request = index / 4;
            return Err(format!("SDF repetition {repetition}, first request {request}, data {:?}, actual {:?}, WGSL {:?}",&requests[request*12..request*12+12],&actual[request*4..request*4+4],&expected[request*4..request*4+4]).into());
        }
        assert_eq!(actual.len(), expected.len());
    }
    device.assert_valid()
}
