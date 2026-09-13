//! Typed adapters for validating production math helpers independently of textures.
use super::super::abi::{Interface, Kind};
use super::{buffer, interface, uniform};
use std::collections::BTreeMap;

pub(super) fn get(family: &str, constants: &BTreeMap<String, u32>) -> Interface {
    let entry = match family {
        "blend-math" => "blend_math_words",
        "pixel-math" => "pixel_math_words",
        "geometry-math" => "geometry_math_words",
        "fill-coverage" => "fill_coverage_words",
        _ => unreachable!("caller filters validation families"),
    };
    let mut resources = vec![
        (
            "config",
            uniform(
                0,
                &[
                    ("count", 0, 1),
                    ("pad0", 4, 1),
                    ("pad1", 8, 1),
                    ("pad2", 12, 1),
                ],
                false,
            ),
        ),
        ("source", buffer(1, Kind::Read)),
        ("destination", buffer(2, Kind::Write)),
    ];
    let mut names = vec!["config", "source", "destination"];
    if family == "fill-coverage" {
        resources.push(("segments", buffer(3, Kind::Read)));
        names.push("segments");
    }
    interface(
        [constants["FINE_WORKGROUP_SIZE"], 1, 1],
        &resources,
        &[(entry, &names)],
    )
}
