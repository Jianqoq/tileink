use super::*;

#[test]
fn color_matrices_preserve_identity_alpha_and_arbitrary_coefficients() {
    use usvg::filter::ColorMatrixKind;
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        1.0, 0.0,
    ];
    assert!(color_matrix_to_filter(&ColorMatrixKind::Matrix(identity.to_vec())).is_none());
    let Filter::ColorMatrix(alpha) =
        color_matrix_to_filter(&ColorMatrixKind::LuminanceToAlpha).unwrap()
    else {
        panic!("luminance-to-alpha must remain a full matrix");
    };
    assert_eq!(&alpha[15..], &[0.2126, 0.7152, 0.0722, 0.0, 0.0]);
    let mut values = identity;
    values[3] = -2.5;
    values[14] = 3.25;
    let Some(Filter::ColorMatrix(actual)) =
        color_matrix_to_filter(&ColorMatrixKind::Matrix(values.to_vec()))
    else {
        panic!("an arbitrary matrix must preserve every coefficient");
    };
    assert_eq!(actual, values);
}

#[test]
fn standard_matrices_keep_their_specialized_filters() {
    use usvg::filter::ColorMatrixKind;
    for (values, grayscale) in [
        (
            [
                0.2126, 0.7152, 0.0722, 0.0, 0.0, 0.2126, 0.7152, 0.0722, 0.0, 0.0, 0.2126, 0.7152,
                0.0722, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
            ],
            true,
        ),
        (
            [
                0.393, 0.769, 0.189, 0.0, 0.0, 0.349, 0.686, 0.168, 0.0, 0.0, 0.272, 0.534, 0.131,
                0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
            ],
            false,
        ),
    ] {
        let lowered = color_matrix_to_filter(&ColorMatrixKind::Matrix(values.to_vec())).unwrap();
        match lowered {
            Filter::Grayscale(amount) if grayscale => assert!((amount - 1.0).abs() < 1.0e-5),
            Filter::Sepia(amount) if !grayscale => assert!((amount - 1.0).abs() < 1.0e-5),
            _ => panic!("standard matrix lost its specialized filter"),
        }
    }
}

#[test]
fn transfer_tables_keep_empty_singleton_and_endpoint_semantics() {
    use usvg::filter::TransferFunction;
    for value in [0.0, 0.375, 1.0] {
        for transfer in [
            TransferFunction::Table(vec![]),
            TransferFunction::Discrete(vec![]),
        ] {
            assert!(transfer_is_identity(&transfer));
            assert_eq!(transfer_function_value(&transfer, value), value);
        }
        assert_eq!(table_transfer_value(&[0.25], value), 0.25);
        assert_eq!(discrete_transfer_value(&[0.25], value), 0.25);
    }
    assert_eq!(table_transfer_value(&[0.0, 0.5, 1.0], 0.25), 0.25);
    assert_eq!(table_transfer_value(&[0.0, 0.5, 1.0], 1.0), 1.0);
    assert_eq!(discrete_transfer_value(&[0.0, 0.5, 1.0], 1.0 / 3.0), 0.5);
    assert_eq!(discrete_transfer_value(&[0.0, 0.5, 1.0], 1.0), 1.0);
}

#[test]
fn component_transfer_quantizes_channels_independently() {
    use usvg::filter::TransferFunction;
    let table = component_transfer_table(
        &TransferFunction::Linear {
            slope: 2.0,
            intercept: -0.25,
        },
        &TransferFunction::Gamma {
            amplitude: 1.0,
            exponent: 2.0,
            offset: 0.0,
        },
        &TransferFunction::Discrete(vec![1.0, 0.0]),
        &TransferFunction::Identity,
    );
    for i in 0..256 {
        let x = i as f32 / 255.0;
        assert_eq!(
            table[i],
            ((2.0 * x - 0.25).clamp(0.0, 1.0) * 255.0 + 0.5) as u32
        );
        assert_eq!(table[256 + i], (x * x * 255.0 + 0.5) as u32);
        assert_eq!(table[512 + i], if i < 128 { 255 } else { 0 });
        assert_eq!(table[768 + i], i as u32);
    }
}
