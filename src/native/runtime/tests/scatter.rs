use super::{Result, reference};
use crate::native::{
    NativeBackend,
    runtime::{adapter::Adapter, program::Scatter},
};
use crate::render::backend::BatchAdapter;
use crate::render::upload::uniforms::UniformWrites;

fn case(lengths: &[usize]) -> (Scatter, Vec<u8>) {
    // Generate expected writes directly from the requested ranges, independently
    // of the packed descriptor decoding performed by the production shaders.
    let mut words = vec![0u32; 4 + lengths.len() * 4];
    words[0] = words.len() as u32;
    words[1] = lengths.len() as u32;
    let mut destination = vec![0xa5u8; (lengths.iter().sum::<usize>() + lengths.len() + 3) * 4];
    let initial = destination.clone();
    let mut dst = 1;
    let mut src = 0;
    for (index, &len) in lengths.iter().enumerate() {
        words[4 + index * 4] = dst as u32;
        words[5 + index * 4] = src as u32;
        words[6 + index * 4] = len as u32;
        for offset in 0..len {
            let value =
                (index as u32).wrapping_mul(0x9e3779b9) ^ (offset as u32).wrapping_mul(0xa53ca53c);
            words.push(value);
            destination[(dst + offset) * 4..(dst + offset + 1) * 4]
                .copy_from_slice(&value.to_le_bytes());
        }
        dst += len + 1;
        src += len;
    }
    (
        Scatter::new(
            words.iter().flat_map(|w| w.to_le_bytes()).collect(),
            initial,
        )
        .unwrap(),
        destination,
    )
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_range_scatter_matches_production_wgsl_and_cpu() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    // Enable the native validation layers before creating either wgpu device.
    let mut dx12 = Adapter::new(NativeBackend::Dx12, &identity)?;
    let mut vulkan = Adapter::new(NativeBackend::Vulkan, &identity)?;
    let wgpu_dx12 = reference::Reference::new(wgpu::Backends::DX12, &identity)?;
    let wgpu_vulkan = reference::Reference::new(wgpu::Backends::VULKAN, &identity)?;
    let mut cases = vec![
        case(&[]),
        case(&[0]),
        case(&[0, 1, 0, 255, 256, 257, 513, 0]),
    ];
    for count in [1, 2, 63, 64, 65, 127, 128, 129, 257] {
        for len in [0, 1, 255, 256, 257, 513] {
            cases.push(case(&vec![len; count]));
        }
    }
    let mut receipts = Vec::new();
    let mut report = Vec::new();
    for _ in 0..3 {
        for (command, _) in &cases {
            let mut d = dx12.create_encoder("range scatter")?;
            let mut v = vulkan.create_encoder("range scatter")?;
            d.dispatch(command.clone());
            v.dispatch(command.clone());
            receipts.push((
                dx12.submit(d, &UniformWrites::default())
                    .map_err(|e| format!("{e:?}"))?,
                vulkan
                    .submit(v, &UniformWrites::default())
                    .map_err(|e| format!("{e:?}"))?,
            ));
        }
    }
    for (index, (d, v)) in receipts.into_iter().enumerate().rev() {
        let (command, expected) = &cases[index % cases.len()];
        let outputs = [
            d.readback()?.remove(0),
            v.readback()?.remove(0),
            wgpu_dx12.execute_scatter(command)?,
            wgpu_vulkan.execute_scatter(command)?,
        ];
        for (api, output) in outputs.iter().enumerate() {
            assert_eq!(output.len(), expected.len());
            assert!(
                output == expected,
                "range scatter case {} API {api}: first differing byte {:?}",
                index % cases.len(),
                output.iter().zip(expected).position(|(a, b)| a != b)
            );
        }
        use sha2::Digest;
        let digest = |bytes: &[u8]| {
            sha2::Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        report.push(serde_json::json!({"case":index%cases.len(),"repetition":index/cases.len(),"workgroups":command.workgroups(),"bytes":expected.len(),"expected_sha256":digest(expected),"outputs":outputs.iter().zip(["native-dx12","native-vulkan","wgpu-dx12","wgpu-vulkan"]).map(|(bytes,route)|serde_json::json!({"route":route,"sha256":digest(bytes),"different_bytes":0})).collect::<Vec<_>>()}));
    }
    assert_eq!(dx12.pending_count(), 0);
    assert_eq!(vulkan.pending_count(), 0);
    dx12.assert_valid()?;
    vulkan.assert_valid()?;
    eprintln!(
        "M4 range scatter: {} cases x 3 repetitions x 4 APIs; zero differing bytes",
        cases.len()
    );
    if let Some(path) = std::env::var_os("TILEINK_NATIVE_SCATTER_REPORT") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(
                &serde_json::json!({"physical_gpu_luid":identity,"cases":cases.len(),"repetitions":3,"routes":4,"frames":report}),
            )?,
        )?;
    }
    Ok(())
}
