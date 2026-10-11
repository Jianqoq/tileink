use super::*;
use peniko::{
    ColorStop, ColorStops, Gradient, GradientKind, LinearGradientPosition,
    color::{AlphaColor, ColorSpaceTag, HueDirection},
};

#[test]
fn fixed_brush_parameters_round_trip_and_truncated_payloads_are_rejected() {
    let transform = [2.0, 3.0, 4.0, 5.0, -6.0, 7.0];
    let brushes = [
        Brush::Linear(LinearGradient {
            start: [-2.0, 3.0],
            end: [4.0, 5.0],
            transform,
            extend: Extend::Reflect,
            ramp: Rc::from([0xff112233, 0xff445566]),
        }),
        Brush::Radial(RadialGradient {
            start_center: [-2.0, 3.0],
            end_center: [4.0, 5.0],
            start_radius: 6.0,
            end_radius: 7.0,
            transform,
            extend: Extend::Repeat,
            ramp: Rc::from([0xff112233, 0xff445566]),
        }),
        Brush::four_corner(
            kurbo::Rect::new(-2.0, 3.0, 4.0, 5.0),
            [
                peniko::Color::BLACK,
                peniko::Color::WHITE,
                peniko::Color::TRANSPARENT,
                peniko::Color::from_rgb8(40, 80, 160),
            ],
        ),
    ];
    for brush in brushes {
        let mut blob = vec![u32::MAX; 2];
        let (offset, len) = push_encoded_brush(&mut blob, &brush);
        let decoded = decode_encoded_brush(&blob, offset, len).unwrap();
        let mut round_trip = Vec::new();
        push_encoded_brush(&mut round_trip, &decoded);
        assert_eq!(round_trip, blob[offset as usize..]);
        for short in 0..len {
            assert!(decode_encoded_brush(&blob, offset, short).is_none());
        }
        let mut invalid = blob.clone();
        invalid[offset as usize + 2] = len;
        invalid[offset as usize + 3] = u32::MAX;
        assert!(decode_encoded_brush(&invalid, offset, len).is_none());
    }
    let mut blob = Vec::new();
    let (offset, len) = push_encoded_brush(
        &mut blob,
        &Brush::four_corner(
            kurbo::Rect::new(0.0, 0.0, 1.0, 1.0),
            [peniko::Color::WHITE; 4],
        ),
    );
    blob[3] = 3;
    assert!(decode_encoded_brush(&blob, offset, len).is_none());
}

#[test]
fn encoded_brush_descriptors_reject_unknown_values_instead_of_substituting_defaults() {
    let resource = Brush::from_image_key_natural(
        ImageKey::new(7),
        [0.0; 2],
        (2, 2),
        Extend::Pad,
        PatternSampling::Nearest,
        255,
    )
    .unwrap();
    let mesh = Brush::Mesh(
        MeshGradient::new(
            2,
            2,
            [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0], [2.0, 2.0]],
            [peniko::Color::WHITE; 4],
        )
        .unwrap(),
    );
    for (brush, mutations) in [
        (
            Brush::from_gradient_with_ramp_size(&linear_gradient(), 4),
            vec![(1, 3)],
        ),
        (
            resource,
            vec![(1, 3), (8, 2), (7, 256), (ENCODED_BRUSH_HEADER_WORDS, 2)],
        ),
        (mesh, vec![(7, 2)]),
    ] {
        let mut blob = Vec::new();
        let (offset, len) = push_encoded_brush(&mut blob, &brush);
        assert!(decode_encoded_brush(&blob, offset, len).is_some());
        for (word, value) in mutations {
            for invalid in [value, u32::MAX] {
                let mut malformed = blob.clone();
                malformed[word] = invalid;
                assert!(
                    decode_encoded_brush(&malformed, offset, len).is_none(),
                    "word {word} value {invalid}"
                );
            }
        }
    }
}

#[test]
fn valid_resource_descriptors_preserve_scope_sampling_extend_and_opacity() {
    let key = ImageKey::new(0xfedc_ba98_7654_3210);
    for id in [ImageResourceId::renderer(key), ImageResourceId::scene(key)] {
        for extend in [Extend::Pad, Extend::Repeat, Extend::Reflect] {
            for sampling in [PatternSampling::Nearest, PatternSampling::Bilinear] {
                for opacity in [0, 127, 255] {
                    let brush = Brush::Pattern(
                        PatternBrush::new_resource(
                            id,
                            [2.0, 0.0, 0.0, 3.0, 4.0, 5.0],
                            extend,
                            sampling,
                            opacity,
                        )
                        .unwrap(),
                    );
                    let mut words = Vec::new();
                    let (offset, len) = push_encoded_brush(&mut words, &brush);
                    let decoded = decode_encoded_brush(&words, offset, len).unwrap();
                    let mut round_trip = Vec::new();
                    push_encoded_brush(&mut round_trip, &decoded);
                    assert_eq!(round_trip, words);
                }
            }
        }
    }
}

#[test]
fn resource_payload_requires_exactly_one_three_word_image_id() {
    let brush = Brush::from_image_key_natural(
        ImageKey::new(7),
        [0.0; 2],
        (2, 2),
        Extend::Pad,
        PatternSampling::Nearest,
        255,
    )
    .unwrap();
    let mut words = Vec::new();
    let (offset, len) = push_encoded_brush(&mut words, &brush);
    words.push(0);
    words[3] = 4;
    assert!(decode_encoded_brush(&words, offset, len + 1).is_none());
}
fn linear_gradient() -> Gradient {
    let stops = [
        ColorStop::from((0.0, AlphaColor::from_rgb8(255, 0, 0))),
        ColorStop::from((1.0, AlphaColor::from_rgb8(0, 0, 255))),
    ];
    Gradient {
        kind: GradientKind::Linear(LinearGradientPosition::new((0.0, 0.0), (1.0, 0.0))),
        extend: Extend::Pad,
        stops: ColorStops::from(stops.as_slice()),
        interpolation_cs: ColorSpaceTag::Srgb,
        interpolation_alpha_space: InterpolationAlphaSpace::Premultiplied,
        hue_direction: HueDirection::Shorter,
    }
}

#[test]
fn from_gradient_with_ramp_size_uses_dynamic_len() {
    let brush = Brush::from_gradient_with_ramp_size(&linear_gradient(), 33);
    let Brush::Linear(gradient) = brush else {
        panic!("expected linear gradient");
    };
    assert_eq!(gradient.ramp.len(), 33);
}

#[test]
fn from_gradient_automatically_estimates_small_linear_ramp() {
    let brush = Brush::from_gradient(&linear_gradient());
    let Brush::Linear(gradient) = brush else {
        panic!("expected linear gradient");
    };
    assert_eq!(gradient.ramp.len(), MIN_GRADIENT_RAMP_SIZE);
}

#[test]
fn estimated_ramp_size_grows_with_span() {
    let small = estimate_linear_ramp_size([0.0, 0.0], [8.0, 0.0], 2);
    let large = estimate_linear_ramp_size([0.0, 0.0], [512.0, 0.0], 2);
    assert!(large > small, "small={small} large={large}");
}

#[test]
fn estimated_ramp_size_respects_stop_density() {
    let sparse = estimate_linear_ramp_size([0.0, 0.0], [8.0, 0.0], 2);
    let dense = estimate_linear_ramp_size([0.0, 0.0], [8.0, 0.0], 12);
    assert!(dense > sparse, "sparse={sparse} dense={dense}");
}

#[test]
fn encoded_brush_blob_round_trips_variable_payload_brushes() {
    let renderer_key = ImageKey::new(0x1234_5678_9abc_def0);
    let scene_key = ImageKey::new(0x2234_5678_9abc_def0);
    let brushes = [
        Brush::Solid(peniko::Color::from_rgba8(64, 128, 255, 128)),
        Brush::Pattern(
            PatternBrush::new_resource(
                ImageResourceId::renderer(renderer_key),
                [2.0, 0.0, 0.0, 3.0, 5.0, 7.0],
                Extend::Reflect,
                PatternSampling::Bilinear,
                200,
            )
            .unwrap(),
        ),
        Brush::Pattern(
            PatternBrush::new_resource(
                ImageResourceId::scene(scene_key),
                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                Extend::Repeat,
                PatternSampling::Nearest,
                255,
            )
            .unwrap(),
        ),
    ];
    let mut blob = Vec::new();
    let ranges = brushes
        .iter()
        .map(|brush| push_encoded_brush(&mut blob, brush))
        .collect::<Vec<_>>();

    assert_eq!(
        decode_encoded_brush(&blob, ranges[0].0, ranges[0].1).and_then(|brush| brush.solid_color()),
        Some(peniko::Color::from_rgba8(64, 128, 255, 128))
    );
    let Some(Brush::Pattern(resource)) = decode_encoded_brush(&blob, ranges[1].0, ranges[1].1)
    else {
        panic!("expected renderer resource pattern");
    };
    assert_eq!(resource.image_key(), Some(renderer_key));
    assert_eq!(resource.transform, [2.0, 0.0, 0.0, 3.0, 5.0, 7.0]);
    assert_eq!(resource.extend, Extend::Reflect);
    assert_eq!(resource.sampling, PatternSampling::Bilinear);
    assert_eq!(resource.opacity, 200);

    let Some(Brush::Pattern(scene)) = decode_encoded_brush(&blob, ranges[2].0, ranges[2].1) else {
        panic!("expected scene resource pattern");
    };
    assert_eq!(scene.image_resource_id(), ImageResourceId::scene(scene_key));
    assert_eq!(scene.extend, Extend::Repeat);
}
