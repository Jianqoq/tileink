use super::*;

#[test]
fn sdf_enum_fields_reject_unknown_fractional_and_nonfinite_values() {
    for (kind, word, valid_count) in [
        (GPU_SDF_LINE, 6, 3),
        (GPU_SDF_DASH_LINE, 6, 3),
        (GPU_SDF_ARC, 7, 3),
        (GPU_SDF_LINE_SHADOW, 6, 3),
        (GPU_SDF_ARC_SHADOW, 7, 3),
        (GPU_SDF_CALLOUT, 10, 4),
        (GPU_SDF_CALLOUT_STROKE, 10, 4),
        (GPU_SDF_CALLOUT_SHADOW, 10, 4),
        (GPU_SDF_CALLOUT, 12, 2),
        (GPU_SDF_CALLOUT_STROKE, 12, 2),
        (GPU_SDF_CALLOUT_SHADOW, 12, 2),
    ] {
        let shadow = matches!(
            kind,
            GPU_SDF_LINE_SHADOW | GPU_SDF_ARC_SHADOW | GPU_SDF_CALLOUT_SHADOW
        );
        let decode = |words: &[u32]| {
            if shadow {
                decode_sdf_shadow(words, 0, ENCODED_SDF_WORDS as u32).is_some()
            } else {
                decode_sdf(words, 0, ENCODED_SDF_WORDS as u32).is_some()
            }
        };
        let mut words = vec![0; ENCODED_SDF_WORDS];
        words[0] = kind;
        for valid in 0..valid_count {
            words[word] = (valid as f32).to_bits();
            assert!(decode(&words), "kind {kind}, valid {valid}");
        }
        for invalid in [
            -1.0,
            0.25,
            valid_count as f32,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            words[word] = invalid.to_bits();
            assert!(!decode(&words), "kind {kind}, invalid {invalid}");
        }
    }
}

#[test]
fn encoded_sdf_requires_a_complete_record_and_preserves_float_bits() {
    let fields = [
        f32::from_bits(0x7fc0_1234),
        -0.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ];
    let sdf = EncodedSdf {
        kind: GPU_SDF_RECT,
        coords: fields,
        radii: fields,
        stroke: fields,
        shadow: fields,
    };
    let mut blob = vec![u32::MAX; 2];
    let (offset, len) = push_encoded(&mut blob, sdf);
    let decoded = encoded_sdf(&blob, offset, len).unwrap();
    let mut round_trip = Vec::new();
    push_encoded(&mut round_trip, decoded);
    assert_eq!(round_trip, blob[offset as usize..]);
    for short in 0..len {
        assert!(encoded_sdf(&blob, offset, short).is_none());
    }
    assert!(encoded_sdf(&blob, offset, len + 1).is_none());
    assert!(encoded_sdf(&blob, u32::MAX, len).is_none());
    blob[offset as usize] = u32::MAX;
    assert!(decode_sdf(&blob, offset, len).is_none());
    assert!(decode_sdf_shadow(&blob, offset, len).is_none());
}

#[test]
fn encoded_sdf_blob_round_trips_sdf_and_shadow_records() {
    let sdfs = [
        Sdf::Rect(Rect {
            start: Point::new(2.0, 3.0),
            end: Point::new(18.0, 19.0),
            radius: Radius::all(4.0),
        }),
        Sdf::RectStroke(RectStroke {
            rect: Rect {
                start: Point::new(4.0, 5.0),
                end: Point::new(40.0, 41.0),
                radius: Radius {
                    top_left: 1.0,
                    top_right: 2.0,
                    bottom_right: 3.0,
                    bottom_left: 4.0,
                },
            },
            widths: StrokeWidths {
                top: 2.0,
                right: 4.0,
                bottom: 6.0,
                left: 8.0,
            },
        }),
        Sdf::DashLine(DashLine::with_offset(
            Point::new(8.0, 16.5),
            Point::new(32.0, 16.5),
            1.0,
            LineCap::Square,
            4.0,
            3.0,
            1.5,
        )),
        Sdf::Triangle(crate::shared::sdf::triangle::Triangle::new(
            Point::new(4.0, 5.0),
            Point::new(20.0, 12.0),
            Point::new(7.0, 30.0),
            2.5,
        )),
        Sdf::Star(Star::new(
            Point::new(18.0, 19.0),
            8.0,
            3.5,
            1.25,
            -std::f32::consts::FRAC_PI_2,
        )),
        Sdf::StarStroke(StarStroke::new(
            Star::new(Point::new(24.0, 25.0), 10.0, 4.6, 1.0, 0.2),
            2.5,
        )),
        Sdf::Callout(Callout::new(
            peniko::kurbo::Rect::new(4.0, 5.0, 84.0, 45.0),
            8.0,
            CalloutTail::new(CalloutSide::Bottom, 38.0, 10.0, 6.0, 1.5),
        )),
        Sdf::CalloutStroke(CalloutStroke::new(
            Callout::new(
                peniko::kurbo::Rect::new(8.0, 9.0, 92.0, 52.0),
                7.0,
                CalloutTail::hidden(),
            ),
            1.5,
        )),
    ];
    let shadows = [
        SdfShadow::Circle(CircleShadow {
            circle: Circle {
                center: Point::new(20.0, 21.0),
                radius: 8.0,
            },
            options: ShadowOptions::new(2.0, 3.0, 4.0, 0.5),
        }),
        SdfShadow::Rc(ArcShadow {
            arc: Rc::new(
                Point::new(32.0, 32.0),
                10.0,
                0.0,
                std::f32::consts::FRAC_PI_2,
                3.0,
                LineCap::Round,
            ),
            options: ShadowOptions::new(-1.0, 2.0, 5.0, 0.75),
        }),
        SdfShadow::Callout(CalloutShadow {
            callout: Callout::new(
                peniko::kurbo::Rect::new(4.0, 5.0, 84.0, 45.0),
                8.0,
                CalloutTail::new(CalloutSide::Left, 18.0, 10.0, 6.0, 1.5),
            ),
            options: ShadowOptions::new(0.0, 6.0, 16.0, 0.28),
        }),
    ];
    let mut sdf_blob = Vec::new();
    for sdf in sdfs {
        let (offset, len) = push_encoded_sdf(&mut sdf_blob, sdf);
        let decoded = decode_sdf(&sdf_blob, offset, len).expect("decode sdf");
        assert_eq!(encode_sdf(decoded), encode_sdf(sdf));
    }

    let mut shadow_blob = Vec::new();
    for shadow in shadows {
        let (offset, len) = push_encoded_sdf_shadow(&mut shadow_blob, shadow);
        let decoded = decode_sdf_shadow(&shadow_blob, offset, len).expect("decode sdf shadow");
        assert_eq!(encode_sdf_shadow(decoded), encode_sdf_shadow(shadow));
    }
}
