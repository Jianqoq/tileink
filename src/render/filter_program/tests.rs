use super::*;
use std::collections::BTreeSet;

#[derive(Default)]
struct Recorder {
    next: usize,
    live: BTreeSet<usize>,
    kernels: Vec<String>,
    fail_kernel: Option<usize>,
    fail_allocation: Option<usize>,
    active: Option<DamageTiles>,
    work: Option<Vec<u32>>,
    work_changes: Vec<Option<Vec<u32>>>,
}

impl FilterAdapter for Recorder {
    type Work = Vec<u32>;
    fn size(&self) -> (u32, u32) {
        (64, 48)
    }
    fn acquire_scratch(&mut self) -> Option<RenderTargetId> {
        let index = self.next;
        self.next += 1;
        if self.fail_allocation == Some(index) {
            return None;
        }
        assert!(self.live.insert(index));
        Some(RenderTargetId::Scratch(index))
    }
    fn release_scratch(&mut self, target: RenderTargetId) {
        let RenderTargetId::Scratch(index) = target else {
            panic!("released the output target")
        };
        assert!(
            self.live.remove(&index),
            "scratch released twice or never acquired"
        );
    }
    fn encode(&mut self, kernel: FilterKernel<'_>) -> bool {
        self.kernels.push(format!("{kernel:?}"));
        self.fail_kernel != Some(self.kernels.len())
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
fn run(recorder: &mut Recorder, filter: &Filter) -> bool {
    FilterExecutor::new(recorder).apply_filter(
        RenderTargetId::Main,
        bounds(),
        filter,
        None,
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
    assert!(!run(&mut recorder, &filter));
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
    assert!(!run(&mut recorder, &filter));
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
    assert!(!run(&mut recorder, &filter));
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
    assert!(run(&mut recorder, &filter));
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
    assert!(run(&mut successful, &filter));
    assert!(successful.live.is_empty());
    for failure in 1..=successful.kernels.len() {
        let mut recorder = Recorder {
            fail_kernel: Some(failure),
            ..Default::default()
        };
        assert!(
            !run(&mut recorder, &filter),
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
            !run(&mut recorder, &filter),
            "ignored allocation failure {failure}"
        );
        assert!(recorder.live.is_empty(), "leaked on allocation {failure}");
    }
}

#[test]
fn blur_skips_zero_axes_and_morphology_handles_canvas_limits() {
    let mut recorder = Recorder::default();
    assert!(run(
        &mut recorder,
        &Filter::Blur {
            std_dev_x: 0.0,
            std_dev_y: 0.0,
            sampling: filter_model::BlurSampling::FULL_RES
        }
    ));
    assert_eq!(recorder.next, 0);
    assert!(run(
        &mut recorder,
        &Filter::Blur {
            std_dev_x: 2.0,
            std_dev_y: 0.0,
            sampling: filter_model::BlurSampling::FULL_RES
        }
    ));
    assert!(recorder.kernels[0].contains("axis: 0"));
    assert!(recorder.kernels[1].starts_with("CopyRegionToTarget"));
    assert!(recorder.live.is_empty());
    let mut recorder = Recorder::default();
    assert!(run(
        &mut recorder,
        &Filter::Morphology {
            radius_x: 32.0,
            radius_y: 1.0,
            operator: filter_model::MorphologyOperator::Erode
        }
    ));
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
        assert!(run(&mut successful, &effect));
        assert!(successful.live.is_empty());
        for failure in 1..=successful.kernels.len() {
            let mut recorder = Recorder {
                fail_kernel: Some(failure),
                ..Default::default()
            };
            assert!(
                !run(&mut recorder, &effect),
                "ignored {effect:?} kernel {failure}"
            );
            assert_eq!(recorder.kernels.len(), failure, "encoded after failure");
            assert!(recorder.live.is_empty());
        }
    }
}

mod work;
