use super::*;

fn active_recorder() -> Recorder {
    let mut active = DamageTiles::new((64, 48));
    active.add_bounds(Bounds::new(16, 16, 32, 32));
    Recorder {
        work: Some(active.list().to_vec()),
        active: Some(active),
        ..Default::default()
    }
}

#[test]
fn downsampled_blur_uses_low_work_then_restores_output_work_on_every_exit() {
    let filter = Filter::Blur {
        std_dev_x: 4.0,
        std_dev_y: 6.0,
        sampling: filter_model::BlurSampling {
            factor: 2,
            ..filter_model::BlurSampling::FULL_RES
        },
    };
    for failure in [None, Some(1), Some(2), Some(3), Some(4)] {
        let mut recorder = active_recorder();
        recorder.fail_kernel = failure;
        assert_eq!(run(&mut recorder, &filter), failure.is_none());
        assert_eq!(recorder.work, Some(vec![5]));
        assert_eq!(recorder.work_changes, [Some(vec![0]), Some(vec![5])]);
        assert!(recorder.live.is_empty());
        if let Some(failure) = failure {
            assert_eq!(recorder.kernels.len(), failure);
        } else {
            assert!(recorder.kernels[0].starts_with("DownsampleRegion"));
            assert!(recorder.kernels[1].contains("std_dev: 2.0, axis: 0"));
            assert!(recorder.kernels[2].contains("std_dev: 3.0, axis: 1"));
            assert!(recorder.kernels[3].starts_with("UpsampleRegion"));
        }
    }
}

#[test]
fn partial_blur_expands_vertical_halo_without_changing_output_work() {
    for failure in [None, Some(1), Some(2)] {
        let mut recorder = active_recorder();
        recorder.fail_kernel = failure;
        let ok = FilterExecutor::new(&mut recorder).apply_blur_from_source_partial(
            RenderTargetId::Main,
            RenderTargetId::Scratch(99),
            Bounds::new(16, 16, 32, 32),
            bounds(),
            2.0,
            3.0,
        );
        assert_eq!(ok, failure.is_none());
        assert_eq!(recorder.work_changes, [Some(vec![1, 5, 9]), Some(vec![5])]);
        assert!(
            recorder.kernels[0]
                .contains(&format!("output_bounds: {:?}", Bounds::new(16, 7, 32, 41)))
        );
        assert_eq!(recorder.work, Some(vec![5]));
        assert!(recorder.live.is_empty());
    }
}

#[test]
fn partial_glass_restores_incremental_state_even_when_a_blur_pass_fails() {
    let glass = filter_model::RectLiquidGlass {
        blur_radius: 6,
        ..Default::default()
    };
    let region = filter_model::rect_liquid_glass_region(None, bounds());
    for failure in [None, Some(1), Some(2), Some(3)] {
        let mut recorder = active_recorder();
        recorder.fail_kernel = failure;
        let ok = FilterExecutor::new(&mut recorder).apply_liquid_glass_from_source_partial(
            RenderTargetId::Main,
            RenderTargetId::Scratch(99),
            Bounds::new(16, 16, 32, 32),
            bounds(),
            glass,
            region,
        );
        assert_eq!(ok, failure.is_none());
        assert_eq!(recorder.active.as_ref().unwrap().list(), &[5]);
        assert_eq!(recorder.work, Some(vec![5]));
        assert_eq!(recorder.work_changes.first(), Some(&None));
        assert_eq!(recorder.work_changes.last(), Some(&Some(vec![5])));
        assert!(recorder.live.is_empty());
        if let Some(failure) = failure {
            assert_eq!(recorder.kernels.len(), failure);
        }
    }
}

#[test]
fn glass_selects_sampled_or_materialized_blur_and_releases_failed_outputs() {
    let region = crate::shared::layer::region::Region::rect(
        peniko::kurbo::Rect::new(0.0, 0.0, 64.0, 48.0),
        crate::shared::sdf::rect::Radius::ZERO,
    );
    for materialized in [false, true] {
        let glass = filter_model::RectLiquidGlass {
            blur_radius: 6,
            blur_sampling: filter_model::BlurSampling {
                factor: 2,
                ..filter_model::BlurSampling::FULL_RES
            },
            refraction_dispersion: 0.0,
            fresnel_factor: 0.0,
            glare_factor: if materialized { 1.0 } else { 0.0 },
            ..Default::default()
        };
        let count = if materialized { 6 } else { 5 };
        for failure in std::iter::once(None).chain((1..=count).map(Some)) {
            let mut recorder = active_recorder();
            recorder.fail_kernel = failure;
            let ok = FilterExecutor::new(&mut recorder)
                .apply_downsampled_liquid_glass_rect_composite(
                    RenderTargetId::Main,
                    bounds(),
                    glass,
                    &region,
                );
            assert_eq!(ok, failure.is_none());
            assert_eq!(recorder.work, Some(vec![5]));
            assert!(recorder.live.is_empty());
            if let Some(failure) = failure {
                assert_eq!(recorder.kernels.len(), failure);
            } else {
                assert_eq!(recorder.kernels.len(), count);
                assert_eq!(
                    recorder
                        .kernels
                        .iter()
                        .any(|item| item.starts_with("UpsampleRegion")),
                    materialized
                );
                assert!(
                    recorder
                        .kernels
                        .last()
                        .unwrap()
                        .starts_with("RectLiquidGlassCompositeRegion")
                );
            }
        }
    }
}
