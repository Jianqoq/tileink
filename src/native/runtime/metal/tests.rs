use super::super::program::{Params, Probe};
use super::*;
#[path = "../tests/cases.rs"]
mod cases;
#[path = "../tests/scan_cases.rs"]
mod scan_cases;

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn production_adapter_matches_all_probe_oracles_and_retires_reverse_readbacks() -> Result<()> {
    let mut device = Metal::with_options(&crate::NativeContextOptions {
        physical_adapter: None,
        validation: true,
    })?;
    let cases = cases::cases();
    for group in cases.chunks(13) {
        let mut pending = Vec::new();
        for case in group {
            pending.push(device.submit_batch(&[case.dispatch.clone().into()])?);
        }
        assert_eq!(device.pending_count(), group.len());
        for (ticket, case) in pending.iter().zip(group).rev() {
            assert_eq!(device.readback_batch(ticket)?, vec![case.expected.clone()]);
        }
        assert_eq!(device.pending_count(), 0);
    }
    device.assert_valid()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn scatter_and_cumsum_cover_tail_chunks_signed_carries_and_padded_groups() -> Result<()> {
    use super::super::program::{Scatter, cumsum::CumsumPlan};
    let mut device = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for count in [0, 1, 255, 256, 257, 519] {
        let mut source = vec![8, count.min(1), 0, 0, 3, 0, count, 0];
        source.extend((0..count).map(|n| n * 17 + 3));
        if count == 0 {
            source = vec![4, 0, 0, 0];
        }
        let destination = vec![0xa5; (count as usize + 7) * 4];
        let scatter = Scatter::new(bytemuck::cast_slice(&source).to_vec(), destination.clone())?;
        let mut expected = destination;
        if count > 0 {
            expected[12..12 + count as usize * 4]
                .copy_from_slice(bytemuck::cast_slice(&source[8..]));
        }
        let ticket = device.submit_batch(&[scatter.into()])?;
        assert_eq!(device.readback_batch(&ticket)?, vec![expected]);
    }
    let lengths = [256, 1, 0, 19, 255];
    let offsets = [0, 256, 257, 257, 276];
    let values: Vec<i32> = (0..531).map(|i| i % 7 - 3).collect();
    let mut expected = values.clone();
    for range in [0..257, 257..531] {
        let mut sum = 0i32;
        for i in range {
            sum = sum.wrapping_add(values[i]);
            expected[i] = sum;
        }
    }
    let plan = CumsumPlan::new(
        offsets.to_vec(),
        lengths.to_vec(),
        vec![0, 2],
        vec![2, 5],
        values.len(),
    )?;
    let mut batch = ComputeBatch::new();
    let backdrops = batch.buffer(bytemuck::cast_slice(&values).to_vec())?;
    plan.encode(&mut batch, backdrops, 3)?;
    batch.readback(backdrops)?;
    let ticket = device.submit_compute(&batch)?;
    assert_eq!(
        device.readback_batch(&ticket)?,
        vec![bytemuck::cast_slice::<i32, u8>(&expected).to_vec()]
    );
    device.assert_valid()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn texture_upload_readback_strips_row_padding_and_preserves_array_layers() -> Result<()> {
    let mut device = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for width in [1, 15, 16, 17, 257] {
        for layers in [1, 3] {
            let bytes: Vec<u8> = (0..width * 7 * layers * 4)
                .map(|n| (n * 19 + 7) as u8)
                .collect();
            let mut batch = ComputeBatch::new();
            let image = if layers == 1 {
                batch.texture_rgba8([width, 7], bytes.clone())?
            } else {
                batch.texture_array_rgba8([width, 7, layers], bytes.clone())?
            };
            batch.readback(image)?;
            let ticket = device.submit_compute(&batch)?;
            assert_eq!(device.readback_batch(&ticket)?, vec![bytes]);
        }
    }
    device.assert_valid()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn public_renderer_clears_empty_frames_at_tile_and_row_pitch_boundaries() -> Result<()> {
    let context = crate::NativeContext::new(
        crate::NativeBackend::Metal,
        &crate::NativeContextOptions {
            validation: true,
            ..Default::default()
        },
    )?;
    for (width, height) in [(1, 1), (15, 17), (17, 15), (257, 3)] {
        let mut renderer = crate::NativeRenderer::with_context(&context, width, height)?;
        let canvas = crate::Canvas::new(width, height, 1.0);
        for color in [
            peniko::Color::TRANSPARENT,
            peniko::Color::from_rgba8(170, 90, 30, 128),
            peniko::Color::BLACK,
        ] {
            renderer.set_clear_color(color);
            let image = renderer.render_to_image(&canvas)?.readback()?;
            let expected = crate::shared::image::premul_color_to_rgba8_pack(color);
            assert!(image.pixels.iter().all(|&pixel| pixel == expected));
            assert_eq!(image.pixels.len(), width as usize * height as usize);
        }
    }
    context.check_validation()?;
    Ok(())
}

#[path = "../tests/coarse_cases.rs"]
mod coarse_cases;

#[path = "../tests/coarse_count_gpu.rs"]
mod coarse_count;
mod coarse_routes {
    use super::*;
    pub(super) struct Routes(std::cell::RefCell<Metal>);
    impl Routes {
        pub(super) fn new() -> Result<Self> {
            Ok(Self(std::cell::RefCell::new(Metal::with_options(
                &crate::NativeContextOptions {
                    validation: true,
                    ..Default::default()
                },
            )?)))
        }
        pub(super) fn check(
            &self,
            batch: &ComputeBatch,
            expected: &[Vec<u8>],
            case: &str,
        ) -> Result<()> {
            let mut device = self.0.borrow_mut();
            let ticket = device.submit_compute(batch)?;
            let actual = device.readback_batch(&ticket)?;
            assert_eq!(actual.len(), expected.len(), "{case}");
            for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
                let diff = a.iter().zip(b).position(|(a, b)| a != b);
                assert!(
                    a.len() == b.len() && diff.is_none(),
                    "{case}: buffer {i}, first different byte {diff:?}"
                );
            }
            Ok(())
        }
        pub(super) fn validate(&self) -> Result<()> {
            self.0.borrow().assert_valid()
        }
    }
}

#[path = "../tests/coarse_allocation_cases.rs"]
mod allocation_cases;

#[path = "scan_tests.rs"]
mod scan_tests;

#[path = "coarse_tests.rs"]
mod coarse_tests;

#[path = "../tests/filter_morphology_cases.rs"]
mod filter_morphology_cases;
#[path = "../tests/filter_transfer_cases.rs"]
mod filter_transfer_cases;

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn morphology_and_transfer_match_independent_cpu_oracles() -> Result<()> {
    let routes = coarse_routes::Routes::new()?;
    for (batch, expected) in filter_morphology_cases::cases()? {
        routes.check(&batch, &expected, "morphology rational semantics")?;
    }
    let (batch, expected) = filter_transfer_cases::case()?;
    routes.check(&batch, &expected, "transfer integer tables")?;
    routes.validate()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn every_compiled_entry_matches_reflected_binding_contract() -> Result<()> {
    let mut metal = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for shader in crate::native::shaders::SHADER_ARTIFACTS {
        pipeline::ensure(
            &metal.device,
            &mut metal.libraries,
            &mut metal.pipelines,
            shader,
        )?;
    }
    metal.assert_valid()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn reflected_uniform_contract_rejects_wrong_scalar_types_before_dispatch() -> Result<()> {
    let mut metal = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    let original = crate::native::shaders::SHADER_ARTIFACTS
        .iter()
        .find(|shader| shader.entry == "copy_words")
        .unwrap();
    let mut layouts = original.uniforms.to_vec();
    let mut fields = layouts[0].fields.to_vec();
    fields[0].1 = (fields[0].1 + 1) % 3;
    layouts[0].fields = Box::leak(fields.into_boxed_slice());
    let mut wrong = *original;
    wrong.uniforms = Box::leak(layouts.into_boxed_slice());
    let wrong = Box::leak(Box::new(wrong));
    let error = pipeline::ensure(
        &metal.device,
        &mut metal.libraries,
        &mut metal.pipelines,
        wrong,
    )
    .unwrap_err();
    assert!(error.to_string().contains("field offsets/types mismatch"));
    assert!(metal.pipelines.is_empty());
    assert_eq!(metal.pending_count(), 0);
    metal.assert_valid()
}
