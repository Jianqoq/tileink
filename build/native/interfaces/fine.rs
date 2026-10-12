use crate::gpu_constants as constants;
#[path = "../../../src/shared/fine_config.rs"]
mod fine_config;
use super::super::abi::{Interface, Kind};
use super::{buffer, interface, uniform};
use fine_config::FineConfig;
const FINE_CONFIG: &[(&str, u32, u32)] = &[
    ("width", std::mem::offset_of!(FineConfig, width) as u32, 1),
    ("height", std::mem::offset_of!(FineConfig, height) as u32, 1),
    (
        "clear_color",
        std::mem::offset_of!(FineConfig, clear_color) as u32,
        1,
    ),
    (
        "tile_count",
        std::mem::offset_of!(FineConfig, tile_count) as u32,
        1,
    ),
    (
        "tiles_width",
        std::mem::offset_of!(FineConfig, tiles_width) as u32,
        1,
    ),
    (
        "tiles_height",
        std::mem::offset_of!(FineConfig, tiles_height) as u32,
        1,
    ),
    (
        "load_target",
        std::mem::offset_of!(FineConfig, load_target) as u32,
        1,
    ),
    (
        "clip_spill_depth",
        std::mem::offset_of!(FineConfig, clip_spill_depth) as u32,
        1,
    ),
    (
        "group_spill_depth",
        std::mem::offset_of!(FineConfig, group_spill_depth) as u32,
        1,
    ),
    (
        "ptcl_capacity",
        std::mem::offset_of!(FineConfig, ptcl_capacity) as u32,
        1,
    ),
    (
        "paint_sdf_shadow_base",
        std::mem::offset_of!(FineConfig, paint_sdf_shadow_base) as u32,
        1,
    ),
    (
        "paint_brush_base",
        std::mem::offset_of!(FineConfig, paint_brush_base) as u32,
        1,
    ),
    (
        "text_image_base",
        std::mem::offset_of!(FineConfig, text_image_base) as u32,
        1,
    ),
    (
        "text_image_data_base",
        std::mem::offset_of!(FineConfig, text_image_data_base) as u32,
        1,
    ),
    (
        "group_spill_base",
        std::mem::offset_of!(FineConfig, group_spill_base) as u32,
        1,
    ),
    (
        "fine_tile_kind_base",
        std::mem::offset_of!(FineConfig, fine_tile_kind_base) as u32,
        1,
    ),
    (
        "active_tile_count",
        std::mem::offset_of!(FineConfig, active_tile_count) as u32,
        1,
    ),
    (
        "dispatch_width",
        std::mem::offset_of!(FineConfig, dispatch_width) as u32,
        1,
    ),
    (
        "active_tile_list_base",
        std::mem::offset_of!(FineConfig, active_tile_list_base) as u32,
        1,
    ),
    (
        "incremental",
        std::mem::offset_of!(FineConfig, incremental) as u32,
        1,
    ),
];
pub(super) fn gradient() -> Interface {
    interface(
        [constants::get("FINE_WORKGROUP_SIZE"), 1, 1],
        &[
            ("config", uniform(0, FINE_CONFIG, false)),
            ("paint", buffer(3, Kind::Read)),
            ("requests", buffer(9, Kind::Read)),
            ("output", buffer(10, Kind::Write)),
            (
                "request_config",
                uniform(
                    11,
                    &[
                        ("count", 0, 1),
                        ("pad0", 4, 1),
                        ("pad1", 8, 1),
                        ("pad2", 12, 1),
                    ],
                    false,
                ),
            ),
        ],
        &[(
            "gradient_words",
            &["config", "paint", "requests", "output", "request_config"],
        )],
    )
}

#[allow(
    clippy::expect_used,
    reason = "Built-in ABI recipes provide the required entry, resource, field and constant before specialization"
)]
pub(super) fn pattern() -> Interface {
    let mut result = gradient();
    result.resources.insert(
        "image_resource_atlas".into(),
        buffer(12, Kind::TextureArray),
    );
    result
        .resources
        .insert("image_resource_sampler".into(), super::sampler(13));
    let mut bindings = result.entries.remove("gradient_words").expect("Built-in ABI recipes provide the required entry, resource, field and constant before specialization");
    bindings.extend([
        "image_resource_atlas".into(),
        "image_resource_sampler".into(),
    ]);
    result.entries.insert("pattern_words".into(), bindings);
    result
}

#[allow(
    clippy::expect_used,
    reason = "Built-in ABI recipes provide the required entry, resource, field and constant before specialization"
)]
pub(super) fn brush() -> Interface {
    let mut result = pattern();
    let mut bindings = result.entries.remove("pattern_words").expect("Built-in ABI recipes provide the required entry, resource, field and constant before specialization");
    result
        .resources
        .insert("image_resource_textures".into(), super::texture_table());
    bindings.push("image_resource_textures".into());
    result.entries.insert("brush_words".into(), bindings);
    result
}

pub(super) fn text() -> Interface {
    interface(
        [constants::get("FINE_WORKGROUP_SIZE"), 1, 1],
        &[
            ("requests", buffer(9, Kind::Read)),
            ("output", buffer(10, Kind::Write)),
            (
                "request_config",
                uniform(
                    11,
                    &[
                        ("count", 0, 1),
                        ("pad0", 4, 1),
                        ("pad1", 8, 1),
                        ("pad2", 12, 1),
                    ],
                    false,
                ),
            ),
        ],
        &[("text_words", &["requests", "output", "request_config"])],
    )
}

pub(super) fn main() -> Interface {
    interface(
        [constants::get("FINE_WORKGROUP_SIZE"), 1, 1],
        &[
            ("config", uniform(0, FINE_CONFIG, false)),
            ("target", buffer(1, Kind::TextureWrite)),
            ("draws", buffer(2, Kind::Read)),
            ("paint", buffer(3, Kind::Read)),
            ("coarse", buffer(4, Kind::Write)),
            ("segments", buffer(5, Kind::Read)),
            ("text", buffer(6, Kind::Read)),
            ("spills", buffer(7, Kind::Write)),
            ("atlas", buffer(12, Kind::TextureArray)),
            ("image_sampler", super::sampler(13)),
            ("images", super::texture_table()),
        ],
        &[(
            "fine_tile_main",
            &[
                "config",
                "target",
                "draws",
                "paint",
                "coarse",
                "segments",
                "text",
                "spills",
                "atlas",
                "image_sampler",
                "images",
            ],
        )],
    )
}
