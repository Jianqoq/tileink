use super::super::abi::{Field, Interface, Kind, Resource, Scalar};
use super::{buffer, interface};

#[path = "../../../src/shared/progressive_blur_config.rs"]
mod progressive_blur_config;

pub(super) fn get() -> Interface {
    use progressive_blur_config::ProgressiveBlurConfig as Config;
    macro_rules! field {
        ($name:ident, $scalar:ident, $lanes:expr) => {
            Field {
                name: stringify!($name).into(),
                offset: std::mem::offset_of!(Config, $name) as u32,
                scalar: Scalar::$scalar,
                lanes: $lanes,
            }
        };
    }
    let config = Resource {
        count: 1,
        binding: 0,
        kind: Kind::Uniform,
        size: std::mem::size_of::<Config>() as u32,
        internal: false,
        fields: vec![
            field!(output, U32, 4),
            field!(source, U32, 4),
            field!(gradient, F32, 4),
            field!(max_std_dev, F32, 1),
            field!(count, U32, 1),
            field!(step, U32, 1),
            field!(axis, U32, 1),
        ],
    };
    let mut table = buffer(30, Kind::TextureTable);
    table.count = 64;
    let mut sampler = buffer(13, Kind::Sampler);
    sampler.size = 0;
    interface(
        [8, 8, 1],
        &[
            ("image_sampler", sampler),
            ("config", config),
            ("source_texture", buffer(1, Kind::Texture)),
            ("level_metadata", buffer(2, Kind::Read)),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("texture_table", table),
        ],
        &[
            (
                "progressive_blur_reduce",
                &[
                    "config",
                    "source_texture",
                    "level_metadata",
                    "target_texture",
                    "image_sampler",
                ],
            ),
            (
                "progressive_blur_resolve",
                &[
                    "config",
                    "level_metadata",
                    "target_texture",
                    "texture_table",
                    "image_sampler",
                ],
            ),
        ],
    )
}
