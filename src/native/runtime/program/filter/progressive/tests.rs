use super::*;

#[test]
fn pyramid_brackets_sigma_with_small_levels_and_calibrated_variance() {
    let pyramid = levels([511, 257], 32.0);
    assert_eq!(pyramid[1].variance, 0.5);
    assert_eq!(pyramid[2].variance, 2.0);
    assert_eq!(pyramid[3].variance, 7.0);
    assert_eq!(pyramid[2].size, [256, 129]);
    assert!(pyramid.last().unwrap().variance >= 1024.0);
    assert!(pyramid[pyramid.len() - 2].variance < 1024.0);
    let texels: u32 = pyramid.iter().skip(2).map(|l| l.size[0] * l.size[1]).sum();
    assert!(texels < 511 * 257 / 2);
    assert!(levels([1, 1], 65536.0).len() < TABLE_SIZE);
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
