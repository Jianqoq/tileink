use super::reference::{FilterVariant, FineVariant};
use super::*;
use crate::native::runtime::compute::ComputeBatch;
use crate::native::runtime::{
    program::scene::SceneCache,
    renderer::{Execution, Images},
};
use crate::shared::layer::{
    filter::{BlurSampling, COMPONENT_TRANSFER_TABLE_LEN, Filter},
    region::Region,
};
use crate::{Canvas, Radius};
use peniko::{Color, kurbo::Rect};

fn frame_filter_canvas(kind: u32, backdrop: bool) -> Canvas {
    let mut c = Canvas::new(19, 13, 1.0);
    let full = Rect::new(0.0, 0.0, 19.0, 13.0);
    let local = Rect::new(3.0, 2.0, 16.0, 11.0);
    c.push_rect(full, Radius::ZERO, Color::from_rgb8(27, 61, 83));
    let filter = match kind {
        0 => Filter::Chain {
            filters: vec![
                Filter::Brightness(0.75),
                Filter::Invert(0.3),
                Filter::Opacity(0.8),
            ],
            fixed_region: true,
        },
        1 | 2 => Filter::Blur {
            std_dev_x: 1.0,
            std_dev_y: 0.75,
            sampling: if kind == 1 {
                BlurSampling::FULL_RES
            } else {
                BlurSampling::downsampled(2)
            },
        },
        3 => Filter::Flood {
            brush: Color::from_rgba8(140, 27, 93, 181).into(),
        },
        4 => {
            let mut table = Box::new([0u32; COMPONENT_TRANSFER_TABLE_LEN]);
            for (i, v) in table.iter_mut().enumerate() {
                *v = if i < 256 {
                    255 - i as u32
                } else {
                    i as u32 % 256
                };
            }
            Filter::ComponentTransfer(table)
        }
        5 => Filter::DropShadow {
            offset_x: 1.0,
            offset_y: 2.0,
            std_dev: 0.5,
            brush: Color::from_rgba8(11, 71, 141, 213).into(),
        },
        _ => advanced_filter(kind),
    };
    let region = Region::rect(if kind == 4 { full } else { local }, Radius::ZERO);
    if backdrop {
        c.push_backdrop_layer(filter, region);
    } else {
        c.push_filter_layer(filter, region);
    }
    // Different colors and sibling operations expose order and stale context state.
    c.push_rect(
        Rect::new(4.0, 3.0, 11.0, 9.0),
        Radius::ZERO,
        Color::from_rgba8(190, 53, 22, 211),
    );
    c.push_filter_layer(
        if kind == 6 {
            convolution(0.2)
        } else if kind == 9 {
            noise_graph(31)
        } else {
            Filter::Opacity(0.5)
        },
        Region::rect(Rect::new(7.0, 4.0, 13.0, 10.0), Radius::ZERO),
    );
    c.push_rect(
        Rect::new(7.0, 4.0, 13.0, 10.0),
        Radius::ZERO,
        Color::from_rgb8(37, 181, 69),
    );
    c.pop_layer();
    c.pop_layer();
    c.push_rect(
        Rect::new(0.0, 0.0, 2.0, 13.0),
        Radius::ZERO,
        Color::from_rgb8(9, 23, 247),
    );
    c
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_frame_filters_backdrops_and_local_contexts_preserve_pixels() -> Result<()> {
    let routes = super::fine_fixture::routes()?;
    let mut cache = SceneCache::default();
    for kind in 0..11 {
        for backdrop in [false, true] {
            if kind == 10 && !backdrop {
                continue;
            }
            let canvas = frame_filter_canvas(kind, backdrop);
            let expected = routes.canvas_reference(&canvas)?;
            let mut batch = ComputeBatch::new();
            let upload = Default::default();
            let images = Images::record(&mut batch, &upload)?;
            let target = Execution::record(
                &mut cache,
                &mut batch,
                &canvas,
                &images,
                None,
                crate::native::runtime::renderer::FrameOptions {
                    chunked: kind % 2 == 0,
                    clear_color: 0,
                    ..Default::default()
                },
                65535,
            )?;
            assert!(batch.outputs().is_empty());
            batch.readback(target)?;
            for fine in FineVariant::ALL {
                routes.check_render(
                    &batch,
                    &expected,
                    &format!("frame filter {kind} backdrop {backdrop}"),
                    FilterVariant {
                        portable: fine.portable,
                        texture_table: fine.texture_table,
                    },
                    fine,
                )?;
            }
        }
    }
    routes.validate()
}

use crate::shared::{bounds::Bounds, layer::filter::*};
fn convolution(bias: f32) -> Filter {
    Filter::ConvolveMatrix(ConvolveMatrix {
        columns: 3,
        rows: 1,
        target_x: 1,
        target_y: 0,
        data: vec![0.25, 0.5, 0.25],
        divisor: 1.0,
        bias,
        edge_mode: ConvolveEdgeMode::Duplicate,
        preserve_alpha: false,
    })
}
fn noise(seed: i32, region: Bounds) -> FilterPrimitive {
    FilterPrimitive {
        input: FilterInput::SourceGraphic,
        input2: None,
        region,
        kind: FilterPrimitiveKind::Turbulence(Turbulence {
            base_frequency_x: 0.19,
            base_frequency_y: 0.27,
            num_octaves: 2,
            seed,
            stitch_tiles: true,
            kind: TurbulenceKind::FractalNoise,
            linear_rgb: false,
            transform_x: 1.0,
            transform_y: 2.0,
            scale_x: 1.0,
            scale_y: 1.0,
            tile_x: region.x0 as f32,
            tile_y: region.y0 as f32,
            tile_width: region.width() as f32,
            tile_height: region.height() as f32,
        }),
    }
}
fn noise_graph(seed: i32) -> Filter {
    let region = Bounds::new(3, 2, 16, 11);
    let primitive = |input, input2, kind| FilterPrimitive {
        input,
        input2,
        region,
        kind,
    };
    Filter::Graph {
        fixed_region: true,
        primitives: vec![
            noise(seed, region),
            noise(seed + 19, region),
            primitive(
                FilterInput::Primitive(0),
                Some(FilterInput::Primitive(1)),
                FilterPrimitiveKind::Blend {
                    mode: peniko::Mix::Screen,
                },
            ),
            primitive(
                FilterInput::SourceGraphic,
                Some(FilterInput::Primitive(2)),
                FilterPrimitiveKind::DisplacementMap(DisplacementMap {
                    scale_x: 2.0,
                    scale_y: 3.0,
                    x_channel: ColorChannel::R,
                    y_channel: ColorChannel::G,
                    linear_rgb: true,
                }),
            ),
            primitive(
                FilterInput::Primitive(3),
                None,
                FilterPrimitiveKind::Tile {
                    source_region: Bounds::new(4, 3, 9, 8),
                },
            ),
            primitive(
                FilterInput::Primitive(4),
                Some(FilterInput::Primitive(2)),
                FilterPrimitiveKind::Composite {
                    operator: CompositeOperator::Arithmetic {
                        k1: 0.2,
                        k2: 0.4,
                        k3: 0.3,
                        k4: 0.1,
                    },
                },
            ),
        ],
    }
}
fn advanced_filter(kind: u32) -> Filter {
    match kind {
        6 => Filter::Chain {
            filters: vec![convolution(0.1), convolution(-0.05)],
            fixed_region: true,
        },
        7 => Filter::DiffuseLighting(DiffuseLighting {
            surface_scale: 2.0,
            diffuse_constant: 1.1,
            lighting_color: [0.7, 0.9, 0.3],
            light_source: LightSource::Point {
                x: 14.0,
                y: 6.0,
                z: 8.0,
            },
        }),
        8 => Filter::SpecularLighting(SpecularLighting {
            surface_scale: 2.0,
            specular_constant: 0.9,
            specular_exponent: 2.0,
            lighting_color: [0.3, 0.7, 0.9],
            light_source: LightSource::Spot {
                x: 10.0,
                y: 2.0,
                z: 12.0,
                points_at_x: 6.0,
                points_at_y: 5.0,
                points_at_z: 0.0,
                specular_exponent: 1.0,
                limiting_cone_angle: Some(55.0),
            },
        }),
        9 => noise_graph(7),
        10 => Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 3,
            blur_sampling: BlurSampling::downsampled(2),
            ..Default::default()
        }),
        _ => unreachable!(),
    }
}
