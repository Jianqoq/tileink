//! Exact four-route cumsum verification, including intermediate values and guards.
use super::{Result, reference};
use crate::{
    NativeBackend,
    native::runtime::{adapter::Adapter, compute::ComputeBatch, program::cumsum::CumsumPlan},
    shared::gpu_constants::CUMSUM_CHUNK_SIZE,
};

fn case(rows: &[usize], seed: u32, limit: u32) -> Result<(ComputeBatch, Vec<Vec<u8>>)> {
    let mut initial = vec![0x12345678i32; 7];
    let mut expected = initial.clone();
    let mut offsets = Vec::new();
    let mut lengths = Vec::new();
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    let mut totals = Vec::new();
    let mut carries = Vec::new();
    let mut rng = seed;
    for &len in rows {
        starts.push(offsets.len() as u32);
        let mut row_carry = 0i32;
        // A zero-length logical row still has one empty chunk, matching partition semantics.
        for chunk in 0..len.div_ceil(CUMSUM_CHUNK_SIZE as usize).max(1) {
            let n = len
                .saturating_sub(chunk * CUMSUM_CHUNK_SIZE as usize)
                .min(CUMSUM_CHUNK_SIZE as usize);
            offsets.push(initial.len() as u32);
            lengths.push(n as u32);
            carries.push(row_carry);
            let mut total = 0i32;
            for lane in 0..n {
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                let value = match lane % 7 {
                    0 => i32::MAX,
                    1 => 1,
                    2 => i32::MIN,
                    3 => -1,
                    _ => rng as i32,
                };
                initial.push(value);
                total = total.wrapping_add(value);
                row_carry = row_carry.wrapping_add(value);
                expected.push(row_carry);
            }
            totals.push(total);
            initial.extend([0x12345678; 3]);
            expected.extend([0x12345678; 3]);
        }
        ends.push(offsets.len() as u32);
    }
    let bytes = |words: &[i32]| {
        words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let mut batch = ComputeBatch::new();
    let backdrops = batch.buffer(bytes(&initial))?;
    batch.readback(backdrops)?;
    let plan = CumsumPlan::new(offsets, lengths, starts, ends, initial.len())?;
    let mut expected = vec![bytes(&expected)];
    if let Some(output) = plan.encode(&mut batch, backdrops, limit)? {
        batch.readback(output.totals)?;
        batch.readback(output.offsets)?;
        if rows.len() == totals.len() {
            carries.fill(0);
        }
        expected.extend([bytes(&totals), bytes(&carries)]);
    }
    Ok((batch, expected))
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_cumsum_keeps_intermediates_and_matches_wrapping_cpu_prefix() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    // Enable D3D12 diagnostics before creating any wgpu D3D12 device.
    let dx12 = Adapter::new(NativeBackend::Dx12, &identity)?;
    let vulkan = Adapter::new(NativeBackend::Vulkan, &identity)?;
    let references = [
        reference::Reference::new(wgpu::Backends::DX12, &identity)?,
        reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
    ];
    let rows: Vec<Vec<usize>> = vec![
        vec![],
        vec![0],
        vec![1],
        vec![255],
        vec![256],
        vec![257],
        vec![513],
        vec![1025],
        vec![0, 1, 257, 0, 513],
        vec![1; 257],
        vec![256, 256, 256],
        vec![65, 1025, 3, 257],
    ];
    let mut compared = 0;
    let mut report = Vec::new();
    for repetition in 0..3 {
        let mut submitted = Vec::new();
        for (index, rows) in rows.iter().enumerate() {
            let chunk_count = rows
                .iter()
                .map(|n| n.div_ceil(CUMSUM_CHUNK_SIZE as usize).max(1))
                .sum::<usize>();
            let limit = if chunk_count <= 9 { 3 } else { 65535 };
            let (batch, expected) = case(rows, 0x97531 + repetition, limit)?;
            let submit_error = |e| format!("native compute submission failed: {e:?}");
            let d = dx12.submit_compute(&batch).map_err(submit_error)?;
            let v = vulkan.submit_compute(&batch).map_err(submit_error)?;
            for (route, reference) in references.iter().enumerate() {
                let actual = reference.execute_compute(&batch)?;
                report.push(receipt(
                    index,
                    repetition,
                    ["wgpu-dx12", "wgpu-vulkan"][route],
                    &actual,
                ));
                assert_eq!(
                    actual, expected,
                    "wgpu route {route}, case {index}, repetition {repetition}"
                );
                compared += 1;
            }
            submitted.push((index, d, v, expected));
        }
        // Later completion must retain earlier unread results and all GPU intermediates.
        for (index, d, v, expected) in submitted.into_iter().rev() {
            let actual_d = d.readback()?;
            let actual_v = v.readback()?;
            report.push(receipt(index, repetition, "native-dx12", &actual_d));
            report.push(receipt(index, repetition, "native-vulkan", &actual_v));
            assert_eq!(actual_d, expected, "native DX12 case {index}");
            assert_eq!(actual_v, expected, "native Vulkan case {index}");
            compared += 2;
        }
    }
    dx12.assert_valid()?;
    vulkan.assert_valid()?;
    if let Some(path) = std::env::var_os("TILEINK_NATIVE_CUMSUM_REPORT") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "physical_gpu_luid":identity,"cases":rows,"repetitions":3,"routes":4,
                "algorithm_chunk_size":CUMSUM_CHUNK_SIZE,"different_bytes":0,"outputs":report,
                "validation":"passed before context teardown; separate runtime lifecycle tests also run"
            }))?,
        )?;
    }
    eprintln!(
        "M4 cumsum: {compared} four-API outputs exactly match CPU, including totals, carries and guards"
    );
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn compute_uniform_storage_alias_preserves_all_read_states() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let device = Adapter::new(NativeBackend::Dx12, &identity)?;
    let mut batch = ComputeBatch::new();
    let words = |values: &[u32]| values.iter().flat_map(|v| v.to_le_bytes()).collect();
    let shared = batch.buffer(words(&[1, 1, 0, 0]))?;
    let backdrops = batch.buffer(words(&[11, 7]))?;
    let totals = batch.buffer(words(&[0]))?;
    // SAFETY: one chunk, length one, offset one, two backdrop words. All shared
    // bindings are read-only; the same allocation is intentionally a CBV and SRV.
    unsafe {
        batch.dispatch(
            "cumsum_prefix_chunks",
            &[
                (0, shared),
                (1, shared),
                (2, shared),
                (5, backdrops),
                (6, totals),
            ],
            [1, 1, 1],
        )?;
    }
    batch.readback(backdrops)?;
    batch.readback(totals)?;
    let receipt = device
        .submit_compute(&batch)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(receipt.readback()?, vec![words(&[11, 7]), words(&[7])]);
    device.assert_valid()?;
    Ok(())
}

fn receipt(case: usize, repetition: u32, route: &str, outputs: &[Vec<u8>]) -> serde_json::Value {
    use sha2::Digest;
    serde_json::json!({"case":case,"repetition":repetition,"route":route,"buffers":outputs.iter().map(|bytes|{
        serde_json::json!({"bytes":bytes.len(),"sha256":sha2::Sha256::digest(bytes).iter().map(|byte|format!("{byte:02x}")).collect::<String>()})
    }).collect::<Vec<_>>()})
}
