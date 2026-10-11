use super::*;
use std::collections::HashMap;

#[test]
fn mask_weights_preserve_defaults_and_select_the_active_raster_mode() {
    let defaults = TextRasterOptions::new();
    assert_eq!(
        defaults.alpha_mask_embolden.to_bits(),
        0.119_441_95f32.to_bits()
    );
    assert_eq!(
        defaults.subpixel_mask_embolden.to_bits(),
        0.032_732_67f32.to_bits()
    );
    for mode in [
        TextSubpixelMode::None,
        TextSubpixelMode::Rgb,
        TextSubpixelMode::Bgr,
    ] {
        let options = defaults
            .with_subpixel_mode(mode)
            .with_alpha_mask_embolden(0.25)
            .with_subpixel_mask_embolden(0.5);
        assert_eq!(
            options.mask_embolden(),
            if mode == TextSubpixelMode::None {
                0.25
            } else {
                0.5
            }
        );
    }
}

#[test]
fn mask_weight_cache_keys_preserve_bitwise_float_identity() {
    let bits = [0, 0x80000000, 0x3f000000, 0x7fc00001, 0x7fc00002];
    let mut keys = HashMap::new();
    for alpha in bits {
        for lcd in bits {
            let options = TextRasterOptions::new()
                .with_alpha_mask_embolden(f32::from_bits(alpha))
                .with_subpixel_mask_embolden(f32::from_bits(lcd));
            assert_eq!(options, options);
            assert!(keys.insert(options, (alpha, lcd)).is_none());
            assert_eq!(keys.get(&options), Some(&(alpha, lcd)));
        }
    }
    assert_eq!(keys.len(), bits.len() * bits.len());
}
