use super::*;

#[test]
fn all_positive_finite_full_redraw_thresholds_through_one_are_valid() {
    for ratio in [f32::MIN_POSITIVE, 0.25, 0.4, 0.7, 1.0] {
        let config = IncrementalRenderConfig {
            full_redraw_ratio: ratio,
            ..Default::default()
        };
        assert_eq!(config.validate(), config);
    }
}

#[test]
fn invalid_full_redraw_thresholds_are_rejected() {
    for ratio in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -0.1,
        -0.0,
        0.0,
        1.01,
    ] {
        let config = IncrementalRenderConfig {
            full_redraw_ratio: ratio,
            ..Default::default()
        };
        assert!(
            std::panic::catch_unwind(|| config.validate()).is_err(),
            "{ratio}"
        );
    }
}
