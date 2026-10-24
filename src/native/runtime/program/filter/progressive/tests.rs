use super::*;

#[test]
fn pyramid_delays_downsampling_and_bounds_resources() {
    for quality in [
        ProgressiveBlurQuality::Balanced,
        ProgressiveBlurQuality::High,
    ] {
        let pyramid = levels([511, 257], 32.0, quality);
        assert!(
            pyramid
                .iter()
                .skip(1)
                .filter(|l| l.variance < 8.0)
                .all(|l| l.scale == 1.0)
        );
        assert!(pyramid.last().unwrap().variance >= 1024.0);
        assert!(pyramid[pyramid.len() - 2].variance < 1024.0);
        for pair in pyramid.windows(2) {
            assert!(pair[1].variance > pair[0].variance);
            assert_eq!(pair[1].size, pair[0].size.map(|n| n.div_ceil(pair[1].step)));
            assert!((pair[1].kernel.iter().map(|t| t[1]).sum::<f32>() - 1.0).abs() < 1e-5);
        }
        assert!(levels([1, 1], 65536.0, quality).len() <= TABLE_SIZE);
        assert_eq!(levels([1, 7], 1.0, quality).len(), 1);
    }
    assert!(
        levels([511, 257], 32.0, ProgressiveBlurQuality::High).len()
            > levels([511, 257], 32.0, ProgressiveBlurQuality::Balanced).len()
    );
}

#[test]
fn zero_blur_is_identity_and_invalid_parameters_record_no_work() {
    let mut batch = ComputeBatch::new();
    let target = batch.texture_rgba8([3, 5], vec![255; 60]).unwrap();
    let mut blur =
        ProgressiveBlur::new(peniko::kurbo::Point::ZERO, peniko::kurbo::Point::ZERO, 0.0);
    encode(&mut batch, target, [3, 5], Bounds::canvas(3, 5), blur).unwrap();
    assert!(batch.commands().is_empty());
    blur.max_std_dev = f32::NAN;
    assert!(encode(&mut batch, target, [3, 5], Bounds::canvas(3, 5), blur).is_err());
    assert!(batch.commands().is_empty());
}

#[test]
fn source_halo_contains_every_kernel_and_reconstruction_sample() {
    for quality in [
        ProgressiveBlurQuality::Balanced,
        ProgressiveBlurQuality::High,
    ] {
        for exponent in -4..=64 {
            let sigma = 2.0_f32.powf(exponent as f32 / 4.0).min(65536.0);
            let pyramid = levels([511, 257], sigma, quality);
            let mut support = 0.0;
            for pair in pyramid.windows(2) {
                let l = &pair[1];
                let center = (l.step - 1) as f32 * 0.5;
                let radius = l
                    .kernel
                    .iter()
                    .map(|t| (t[0] - center).abs().ceil() + 1.0)
                    .fold(0.0, f32::max);
                support += pair[0].scale * radius;
            }
            support = (support + pyramid.last().unwrap().scale).max(3.0);
            let blur = ProgressiveBlur::new(
                peniko::kurbo::Point::ZERO,
                peniko::kurbo::Point::ZERO,
                sigma,
            )
            .with_quality(quality);
            assert!(
                support <= blur.sample_outset() as f32,
                "{quality:?}, sigma={sigma}: {support}"
            );
        }
    }
}
