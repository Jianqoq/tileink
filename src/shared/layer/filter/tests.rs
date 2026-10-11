use super::*;
fn erode(x: f32, y: f32) -> Filter {
    Filter::Morphology {
        radius_x: x,
        radius_y: y,
        operator: MorphologyOperator::Erode,
    }
}

#[test]
fn erosion_read_dependency_is_independent_of_its_zero_output_expansion() {
    for (x, y, expected) in [
        (1.0, 0.0, 1),
        (1.2, 0.0, 2),
        (0.0, 2.2, 3),
        (0.0, 0.0, 0),
        (-1.0, -2.0, 0),
    ] {
        assert_eq!(filter_outset(&erode(x, y)), 0);
        assert_eq!(dependency_radius(&erode(x, y)), expected);
    }
}

#[test]
fn fixed_chains_and_graphs_preserve_erosion_input_dependencies() {
    let chain = Filter::Chain {
        filters: vec![erode(1.0, 0.0), erode(0.0, 1.2)],
        fixed_region: true,
    };
    assert_eq!(filter_outset(&chain), 0);
    assert_eq!(dependency_radius(&chain), 3);
    let graph = Filter::Graph {
        fixed_region: true,
        primitives: vec![FilterPrimitive {
            input: FilterInput::SourceGraphic,
            input2: None,
            region: Bounds::canvas(32, 32),
            linear_rgb: false,
            kind: FilterPrimitiveKind::Filter(Box::new(chain)),
        }],
    };
    assert_eq!(filter_outset(&graph), 0);
    assert_eq!(dependency_radius(&graph), 3);
}

#[test]
fn dilation_keeps_its_existing_output_and_input_expansion() {
    let filter = Filter::Morphology {
        radius_x: 1.2,
        radius_y: 0.0,
        operator: MorphologyOperator::Dilate,
    };
    assert_eq!(filter_outset(&filter), 2);
    assert_eq!(dependency_radius(&filter), 2);
}

fn convolve(
    columns: u32,
    rows: u32,
    target_x: u32,
    target_y: u32,
    divisor: f32,
    edge_mode: ConvolveEdgeMode,
) -> Filter {
    Filter::ConvolveMatrix(ConvolveMatrix {
        columns,
        rows,
        target_x,
        target_y,
        data: vec![1.0; (columns * rows) as usize],
        divisor,
        bias: 0.0,
        edge_mode,
        preserve_alpha: false,
    })
}

#[test]
fn local_convolution_dependency_covers_every_kernel_offset() {
    for edge in [ConvolveEdgeMode::None, ConvolveEdgeMode::Duplicate] {
        for (cols, rows, x, y, radius) in [(3, 1, 1, 0, 1), (4, 3, 0, 2, 3), (1, 1, 0, 0, 0)] {
            let filter = convolve(cols, rows, x, y, 1.0, edge);
            assert_eq!(filter_outset(&filter), 0);
            assert_eq!(dependency_radius(&filter), radius);
        }
    }
}

#[test]
fn no_op_convolution_has_no_neighbour_dependency() {
    for (cols, rows, divisor) in [(0, 3, 1.0), (3, 0, 1.0), (3, 3, 0.0)] {
        let filter = convolve(cols, rows, 1, 1, divisor, ConvolveEdgeMode::Duplicate);
        assert_eq!(dependency_radius(&filter), 0);
    }
}

#[test]
fn both_lighting_effects_read_one_pixel_for_their_sobel_normal() {
    let light_source = LightSource::Distant {
        azimuth: 0.0,
        elevation: 45.0,
    };
    for filter in [
        Filter::DiffuseLighting(DiffuseLighting {
            surface_scale: 2.0,
            diffuse_constant: 0.5,
            lighting_color: [1.0; 3],
            light_source,
        }),
        Filter::SpecularLighting(SpecularLighting {
            surface_scale: 2.0,
            specular_constant: 0.5,
            specular_exponent: 4.0,
            lighting_color: [1.0; 3],
            light_source,
        }),
    ] {
        assert_eq!(filter_outset(&filter), 0);
        assert_eq!(dependency_radius(&filter), 1);
    }
}

#[test]
fn wrapped_convolution_keeps_the_original_domain_outside_the_visible_target() {
    let filter = convolve(3, 1, 1, 0, 1.0, ConvolveEdgeMode::Wrap);
    let region = Region::Rect {
        rect: peniko::kurbo::Rect::new(-16.0, 0.0, 48.0, 32.0),
        radius: crate::Radius::ZERO,
    };
    let bounds = filter_surface_bounds(&filter, &region, Bounds::canvas(32, 32)).unwrap();
    assert_eq!(bounds.output, Bounds::canvas(32, 32));
    assert_eq!(
        bounds.surface,
        Bounds::new(-16, 0, 48, 32),
        "cropping changes the wrap period and the opposite-edge source"
    );
}

#[test]
fn fixed_chains_and_graphs_keep_a_nested_wrap_domain() {
    let chain = Filter::Chain {
        fixed_region: true,
        filters: vec![
            convolve(3, 1, 1, 0, 1.0, ConvolveEdgeMode::Wrap),
            Filter::Opacity(0.5),
        ],
    };
    let region = Region::Rect {
        rect: peniko::kurbo::Rect::new(-16.0, 0.0, 48.0, 32.0),
        radius: crate::Radius::ZERO,
    };
    let graph = Filter::Graph {
        fixed_region: true,
        primitives: vec![FilterPrimitive {
            input: FilterInput::SourceGraphic,
            input2: None,
            region: Bounds::new(-16, 0, 48, 32),
            linear_rgb: false,
            kind: FilterPrimitiveKind::Filter(Box::new(chain.clone())),
        }],
    };
    for filter in [chain, graph] {
        let bounds = filter_surface_bounds(&filter, &region, Bounds::canvas(32, 32)).unwrap();
        assert_eq!(bounds.surface, Bounds::new(-16, 0, 48, 32));
        assert_eq!(bounds.output, Bounds::canvas(32, 32));
    }
}

fn dependency_radius(filter: &Filter) -> u32 {
    filter_dependency(filter)
        .local_radius()
        .expect("this case has a finite neighbourhood")
}

#[test]
fn a_large_convolution_anchor_cannot_wrap_the_source_footprint_to_empty() {
    let filter = convolve(1, 1, i32::MAX as u32, 0, 1.0, ConvolveEdgeMode::Duplicate);
    let domain = Bounds::canvas(32, 32);
    assert_eq!(
        filter_dependency(&filter).source_bounds(domain, domain),
        domain
    );
    assert_eq!(
        filter_dependency(&filter).affected_output(Bounds::new(0, 0, 1, 1), domain, domain),
        domain
    );
}

#[test]
fn tile_dependency_preserves_distances_across_the_signed_coordinate_domain() {
    let output = Bounds::new(i32::MAX - 8, i32::MAX - 8, i32::MAX, i32::MAX);
    let source = Bounds::new(i32::MIN, i32::MIN, i32::MIN + 8, i32::MIN + 8);
    let filter = Filter::Graph {
        primitives: vec![FilterPrimitive {
            input: FilterInput::SourceGraphic,
            input2: None,
            region: output,
            linear_rgb: false,
            kind: FilterPrimitiveKind::Tile {
                source_region: source,
            },
        }],
        fixed_region: true,
    };
    let dependency = filter_dependency(&filter);
    assert_eq!(
        dependency.local_radius().unwrap() as u64,
        u64::from(u32::MAX - 8)
    );
    assert_eq!(dependency.source_bounds(output, source), source);
    assert_eq!(dependency.affected_output(source, source, output), output);
}

#[test]
fn whole_region_damage_uses_the_unsigned_extent_without_truncation() {
    let filter = convolve(3, 1, 1, 0, 1.0, ConvolveEdgeMode::Wrap);
    let domain = Bounds::new(i32::MIN, -1, i32::MAX, 1);
    assert_eq!(
        filter_dependency(&filter).damage_outset(domain) as u64,
        u64::from(u32::MAX - 1)
    );
}

#[test]
fn combined_filter_radii_do_not_wrap_or_stop_at_the_signed_limit() {
    for (sigma, offset, expected) in [
        (2.0, 4.1, 11u64),
        (1.0e9, 1.0, 3_000_000_001),
        (f32::MAX, f32::MAX, u64::from(u32::MAX)),
    ] {
        let shadow = Filter::DropShadow {
            std_dev: sigma,
            offset_x: offset,
            offset_y: -offset,
            brush: crate::Brush::Solid(peniko::Color::BLACK),
        };
        assert_eq!(filter_outset(&shadow) as u64, expected);
    }
    let glass = RectLiquidGlass {
        blur_radius: u32::MAX,
        ..RectLiquidGlass::default()
    };
    assert_eq!(glass.sample_outset() as u64, u64::from(u32::MAX));
    let filter = convolve(1, 1, u32::MAX, 0, 1.0, ConvolveEdgeMode::Duplicate);
    assert_eq!(
        filter_dependency(&filter).local_radius().unwrap() as u64,
        u64::from(u32::MAX)
    );
}
