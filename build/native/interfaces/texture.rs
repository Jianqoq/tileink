use super::{Interface, Kind, buffer, interface, uniform};
use std::collections::BTreeMap;

pub(super) fn validation(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FINE_WORKGROUP_SIZE"], 1, 1],
        &[
            (
                "config",
                uniform(
                    0,
                    &[
                        ("width", 0, 1),
                        ("height", 4, 1),
                        ("pad0", 8, 1),
                        ("pad1", 12, 1),
                    ],
                    false,
                ),
            ),
            ("source", buffer(1, Kind::Texture)),
            ("destination", buffer(2, Kind::TextureWrite)),
        ],
        &[("texture_flip", &["config", "source", "destination"])],
    )
}

pub(super) fn array(constants: &BTreeMap<String, u32>) -> Interface {
    let mut result = validation(constants);
    result.resources.get_mut("config").unwrap().fields[2].name = "layer".into();
    result.resources.get_mut("source").unwrap().kind = Kind::TextureArray;
    result.entries = [(
        "texture_layer".into(),
        vec!["config".into(), "source".into(), "destination".into()],
    )]
    .into();
    result
}

pub(super) fn sampler(constants: &BTreeMap<String, u32>) -> Interface {
    let mut sampler = buffer(2, Kind::Sampler);
    sampler.size = 0;
    interface(
        [constants["FINE_WORKGROUP_SIZE"], 1, 1],
        &[
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
            ("source", buffer(1, Kind::TextureArray)),
            ("image_sampler", sampler),
            ("requests", buffer(3, Kind::Read)),
            ("destination", buffer(4, Kind::Write)),
            ("dispatch_grid", uniform(31, super::DISPATCH_GRID, true)),
        ],
        &[(
            "sampler_words",
            &[
                "config",
                "source",
                "image_sampler",
                "requests",
                "destination",
                "dispatch_grid",
            ],
        )],
    )
}
