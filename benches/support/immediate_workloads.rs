#[path = "../../examples/common/mod.rs"]
mod common;
#[allow(dead_code)]
#[path = "../../examples/support/retained_dirty_ratio.rs"]
mod dirty;
#[path = "filter_workload.rs"]
mod filter;
#[path = "../../examples/common/numeric_raster_cases.rs"]
mod numeric;
#[path = "root_workload.rs"]
mod root;

pub struct Case {
    pub name: String,
    pub frames: Vec<tileink::Canvas>,
}

pub fn cases() -> Vec<Case> {
    let mut cases: Vec<_> = root::benchmark_scenes()
        .into_iter()
        .map(|(name, canvas, _)| Case {
            name: format!("root-{name}"),
            frames: vec![canvas],
        })
        .collect();
    let dirty_workload = dirty::Workload::new();
    for ratio in dirty::RATIOS {
        cases.push(Case {
            name: format!("dirty-immediate-{:.1}pct", ratio * 100.0),
            frames: dirty_workload.immediate_frames(ratio).into_iter().collect(),
        });
    }
    for (name, fixture) in numeric::CASES.into_iter().chain([(
        "rotated-context",
        "painting/context/with-pattern-and-transform-in-use.svg",
    )]) {
        for width in numeric::WIDTHS {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/svg/tests")
                .join(fixture);
            let (canvas, _, _) = common::load_svg_scene(path, width).unwrap();
            cases.push(Case {
                name: format!("raster-{name}-{width}"),
                frames: vec![canvas],
            });
        }
    }
    for (name, sizes) in [
        ("fixed", &[(1280, 960)][..]),
        (
            "resize",
            &[(1280, 960), (1296, 976), (1288, 968), (1272, 952)][..],
        ),
    ] {
        cases.push(Case {
            name: format!("filter-{name}"),
            frames: sizes.iter().copied().map(filter::scene).collect(),
        });
    }
    cases
}

#[path = "../../examples/support/retained_dimensions.rs"]
mod retained_dimensions;
