use super::*;
use std::collections::BTreeSet;

#[derive(Default)]
struct Recorder {
    next: usize,
    live: BTreeSet<usize>,
    kernels: Vec<String>,
    fail_kernel: Option<usize>,
    fail_allocation: Option<usize>,
    fail_release: Option<usize>,
    active: Option<DamageTiles>,
    work: Option<Vec<u32>>,
    work_changes: Vec<Option<Vec<u32>>>,
}

impl FilterAdapter for Recorder {
    type Error = &'static str;
    type Work = Vec<u32>;
    fn size(&self) -> (u32, u32) {
        (64, 48)
    }
    fn acquire_scratch(&mut self) -> Result<RenderTargetId, Self::Error> {
        let index = self.next;
        self.next += 1;
        if self.fail_allocation == Some(index) {
            return Err("scratch allocation failed");
        }
        assert!(self.live.insert(index));
        Ok(RenderTargetId::Scratch(index))
    }
    fn release_scratch(&mut self, target: RenderTargetId) -> Result<(), Self::Error> {
        let RenderTargetId::Scratch(index) = target else {
            panic!("released the output target")
        };
        assert!(
            self.live.remove(&index),
            "scratch released twice or never acquired"
        );
        if self.fail_release == Some(index) {
            return Err("scratch release failed");
        }
        Ok(())
    }
    fn encode(&mut self, kernel: FilterKernel<'_>) -> Result<(), Self::Error> {
        self.kernels.push(format!("{kernel:?}"));
        if self.fail_kernel == Some(self.kernels.len()) {
            Err("kernel recording failed")
        } else {
            Ok(())
        }
    }
    fn active_tiles(&self) -> Option<&DamageTiles> {
        self.active.as_ref()
    }
    fn filter_work(&self) -> Option<Self::Work> {
        self.work.clone()
    }
    fn set_filter_work(&mut self, work: Option<Self::Work>) {
        self.work_changes.push(work.clone());
        self.work = work;
    }
    fn prepare_filter_tile_work(&mut self, tiles: &[u32]) {
        self.set_filter_work(Some(tiles.to_vec()));
    }
    fn suspend_incremental_filter_work(&mut self) -> Option<DamageTiles> {
        let active = self.active.take();
        if active.is_some() {
            self.set_filter_work(None);
        }
        active
    }
    fn restore_incremental_filter_work(&mut self, active: Option<DamageTiles>) {
        self.active = active;
        self.set_filter_work(self.active.as_ref().map(|tiles| tiles.list().to_vec()));
    }
}

fn bounds() -> Bounds {
    Bounds::canvas(64, 48)
}
fn run(recorder: &mut Recorder, filter: &Filter) -> Result<(), &'static str> {
    FilterExecutor::new(recorder).apply_filter(
        RenderTargetId::Main,
        bounds(),
        filter,
        None,
        false,
        &mut FilterCursors::default(),
    )
}
fn graph(primitives: Vec<filter_model::FilterPrimitive>) -> Filter {
    Filter::Graph {
        primitives,
        fixed_region: false,
    }
}
fn primitive(
    input: filter_model::FilterInput,
    kind: filter_model::FilterPrimitiveKind,
) -> filter_model::FilterPrimitive {
    filter_model::FilterPrimitive {
        input,
        input2: None,
        region: bounds(),
        linear_rgb: false,
        kind,
    }
}

#[test]
fn chain_stops_when_a_color_kernel_cannot_be_recorded() {
    let filter = Filter::Chain {
        filters: vec![
            Filter::Opacity(0.5),
            Filter::Brightness(0.7),
            Filter::Contrast(0.2),
        ],
        fixed_region: false,
    };
    let mut recorder = Recorder {
        fail_kernel: Some(2),
        ..Default::default()
    };
    assert!(run(&mut recorder, &filter).is_err());
    assert_eq!(recorder.kernels.len(), 2);
    assert!(recorder.live.is_empty());
}

#[test]
fn invalid_merge_input_releases_the_new_output() {
    let filter = graph(vec![primitive(
        filter_model::FilterInput::SourceGraphic,
        filter_model::FilterPrimitiveKind::Merge {
            inputs: vec![filter_model::FilterInput::Primitive(3)],
        },
    )]);
    let mut recorder = Recorder::default();
    assert!(run(&mut recorder, &filter).is_err());
    assert!(
        recorder.live.is_empty(),
        "invalid graph input leaked its output scratch"
    );
}

#[test]
fn a_failed_graph_clear_stops_before_copying_stale_pixels() {
    let filter = graph(vec![primitive(
        filter_model::FilterInput::SourceGraphic,
        filter_model::FilterPrimitiveKind::Identity,
    )]);
    let mut recorder = Recorder {
        fail_kernel: Some(1),
        ..Default::default()
    };
    assert!(run(&mut recorder, &filter).is_err());
    assert_eq!(recorder.kernels.len(), 1);
    assert!(recorder.live.is_empty());
}

#[test]
fn graph_reuses_source_alpha_and_preserves_merge_painter_order() {
    use filter_model::{FilterInput as I, FilterPrimitiveKind as P};
    let filter = graph(vec![
        primitive(I::SourceAlpha, P::Identity),
        primitive(I::SourceAlpha, P::Identity),
        primitive(
            I::SourceGraphic,
            P::Merge {
                inputs: vec![I::Primitive(1), I::Primitive(0)],
            },
        ),
    ]);
    let mut recorder = Recorder::default();
    assert!(run(&mut recorder, &filter).is_ok());
    assert_eq!(
        recorder
            .kernels
            .iter()
            .filter(|item| item.starts_with("SourceAlphaToTarget"))
            .count(),
        1
    );
    let merges: Vec<_> = recorder
        .kernels
        .iter()
        .filter(|item| item.starts_with("SourceOverFilterInput"))
        .collect();
    assert_eq!(merges.len(), 2);
    assert!(merges[0].contains("source: Scratch(2)"));
    assert!(merges[1].contains("source: Scratch(1)"));
    assert!(recorder.live.is_empty());
}

#[test]
fn graph_releases_every_allocation_on_each_recording_and_allocation_failure() {
    use filter_model::{FilterInput as I, FilterPrimitiveKind as P};
    let filter = graph(vec![
        primitive(
            I::SourceAlpha,
            P::Filter(Box::new(Filter::Blur {
                std_dev_x: 2.0,
                std_dev_y: 3.0,
                sampling: filter_model::BlurSampling::FULL_RES,
            })),
        ),
        primitive(
            I::SourceGraphic,
            P::Merge {
                inputs: vec![I::Primitive(0), I::SourceAlpha],
            },
        ),
    ]);
    let mut successful = Recorder::default();
    assert!(run(&mut successful, &filter).is_ok());
    assert!(successful.live.is_empty());
    for failure in 1..=successful.kernels.len() {
        let mut recorder = Recorder {
            fail_kernel: Some(failure),
            ..Default::default()
        };
        assert!(
            run(&mut recorder, &filter).is_err(),
            "ignored kernel failure {failure}"
        );
        assert!(recorder.live.is_empty(), "leaked on kernel {failure}");
    }
    for failure in 0..successful.next {
        let mut recorder = Recorder {
            fail_allocation: Some(failure),
            ..Default::default()
        };
        assert!(
            run(&mut recorder, &filter).is_err(),
            "ignored allocation failure {failure}"
        );
        assert!(recorder.live.is_empty(), "leaked on allocation {failure}");
    }
}

#[test]
fn blur_skips_zero_axes_and_morphology_handles_canvas_limits() {
    let mut recorder = Recorder::default();
    assert!(
        run(
            &mut recorder,
            &Filter::Blur {
                std_dev_x: 0.0,
                std_dev_y: 0.0,
                sampling: filter_model::BlurSampling::FULL_RES
            }
        )
        .is_ok()
    );
    assert_eq!(recorder.next, 0);
    assert!(
        run(
            &mut recorder,
            &Filter::Blur {
                std_dev_x: 2.0,
                std_dev_y: 0.0,
                sampling: filter_model::BlurSampling::FULL_RES
            }
        )
        .is_ok()
    );
    assert!(recorder.kernels[0].contains("axis: 0"));
    assert!(recorder.kernels[1].starts_with("CopyRegionToTarget"));
    assert!(recorder.live.is_empty());
    let mut recorder = Recorder::default();
    assert!(
        run(
            &mut recorder,
            &Filter::Morphology {
                radius_x: 32.0,
                radius_y: 1.0,
                operator: filter_model::MorphologyOperator::Erode
            }
        )
        .is_ok()
    );
    assert_eq!(recorder.next, 0);
    assert_eq!(recorder.kernels.len(), 1);
    assert!(recorder.kernels[0].starts_with("ClearRenderRegion"));
}

#[test]
fn effects_stop_at_every_failed_kernel_and_release_scratch() {
    let effects = [
        Filter::ColorMatrix([0.0; 20]),
        Filter::ComponentTransfer(Box::new([0; 1024])),
        Filter::Morphology {
            radius_x: 2.0,
            radius_y: 3.0,
            operator: filter_model::MorphologyOperator::Dilate,
        },
        Filter::DropShadow {
            offset_x: 1.0,
            offset_y: -1.0,
            std_dev: 2.0,
            brush: peniko::Color::BLACK.into(),
        },
    ];
    for effect in effects {
        let mut successful = Recorder::default();
        assert!(run(&mut successful, &effect).is_ok());
        assert!(successful.live.is_empty());
        for failure in 1..=successful.kernels.len() {
            let mut recorder = Recorder {
                fail_kernel: Some(failure),
                ..Default::default()
            };
            assert!(
                run(&mut recorder, &effect).is_err(),
                "ignored {effect:?} kernel {failure}"
            );
            assert_eq!(recorder.kernels.len(), failure, "encoded after failure");
            assert!(recorder.live.is_empty());
        }
    }
}

mod graph_shader;
mod work;

#[test]
fn filter_errors_are_preserved_and_cleanup_does_not_hide_recording_failure() {
    let filter = Filter::Offset { dx: 1.0, dy: 2.0 };
    for (kernel, allocation, release, expected) in [
        (None, Some(0), None, "scratch allocation failed"),
        (Some(1), None, None, "kernel recording failed"),
        (Some(1), None, Some(0), "kernel recording failed"),
        (None, None, Some(0), "scratch release failed"),
    ] {
        let mut r = Recorder {
            fail_kernel: kernel,
            fail_allocation: allocation,
            fail_release: release,
            ..Default::default()
        };
        assert_eq!(run(&mut r, &filter), Err(expected));
        assert!(r.live.is_empty());
    }
}

#[test]
fn graph_releases_published_and_unpublished_outputs_on_every_cleanup_failure() {
    use filter_model::{FilterInput as I, FilterPrimitiveKind as P};
    let filter = graph(vec![
        primitive(
            I::SourceAlpha,
            P::Filter(Box::new(Filter::Offset { dx: 1.0, dy: 0.0 })),
        ),
        primitive(I::Primitive(0), P::Identity),
    ]);
    let mut successful = Recorder::default();
    run(&mut successful, &filter).unwrap();
    for release in 0..successful.next {
        let mut r = Recorder {
            fail_release: Some(release),
            ..Default::default()
        };
        assert_eq!(run(&mut r, &filter), Err("scratch release failed"));
        assert!(r.live.is_empty(), "leaked after release {release}");
    }
}

#[test]
fn direct_blur_inapplicability_is_distinct_from_recording_failure() {
    let region = crate::Region::rect(
        peniko::kurbo::Rect::new(0., 0., 64., 48.),
        crate::Radius::ZERO,
    );
    let mut r = Recorder::default();
    assert_eq!(
        FilterExecutor::new(&mut r).apply_downsampled_blur_rect_composite(
            RenderTargetId::Main,
            bounds(),
            2.,
            2.,
            filter_model::BlurSampling::FULL_RES,
            &region
        ),
        Ok(false)
    );
    assert_eq!(r.next, 0);
    r.fail_allocation = Some(0);
    assert_eq!(
        FilterExecutor::new(&mut r).apply_downsampled_blur_rect_composite(
            RenderTargetId::Main,
            bounds(),
            2.,
            2.,
            filter_model::BlurSampling::downsampled(2),
            &region
        ),
        Err("scratch allocation failed")
    );
}
