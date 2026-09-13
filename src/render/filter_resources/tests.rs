use super::{cursors::FilterCursors, tables::*};
use crate::shared::{gpu_brush::GpuBrushUpload, layer::filter::*};
use crate::{Canvas, Radius, Region};
use peniko::{Color, kurbo::Rect};

fn region() -> Region {
    Region::Rect {
        rect: Rect::new(0.0, 0.0, 17.0, 19.0),
        radius: Radius::ZERO,
    }
}

fn transfer(marker: u32) -> Filter {
    Filter::ComponentTransfer(Box::new([marker; COMPONENT_TRANSFER_TABLE_LEN]))
}

fn draw(canvas: &mut Canvas) {
    canvas.push_rect(Rect::new(0.0, 0.0, 17.0, 19.0), Radius::ZERO, Color::WHITE);
}

fn nested_transfers() -> crate::shared::execution::ExecPlan {
    let mut canvas = Canvas::new(17, 19, 1.0);
    canvas.push_filter_layer(transfer(11), region());
    canvas.push_backdrop_layer(transfer(22), region());
    canvas.push_filter_layer(transfer(33), region());
    draw(&mut canvas);
    for _ in 0..3 {
        canvas.pop_layer();
    }
    canvas.push_filter_layer(transfer(44), region());
    draw(&mut canvas);
    canvas.pop_layer();
    canvas.compile(canvas.root_commands)
}

#[test]
fn filter_tables_follow_backdrop_then_children_then_parent_order() {
    let plan = nested_transfers();
    let upload = FilterTransferUpload::from_plan(&plan);
    let markers: Vec<_> = upload
        .tables
        .chunks_exact(COMPONENT_TRANSFER_TABLE_LEN)
        .map(|table| {
            assert!(table.iter().all(|&value| value == table[0]));
            table[0]
        })
        .collect();
    assert_eq!(markers, [22, 33, 11, 44]);
}

#[test]
fn skipping_nested_filter_work_keeps_following_table_indices_aligned() {
    let plan = nested_transfers();
    let upload = FilterTransferUpload::from_plan(&plan);
    let mut cursor = FilterCursors::default();
    cursor.advance_ops(&plan.ops[..1]);
    let next = cursor.next_transfer_index();
    assert_eq!(next, 3);
    assert_eq!(
        upload.tables[next as usize * COMPONENT_TRANSFER_TABLE_LEN],
        44
    );
    let mut clone = cursor.clone();
    assert_eq!(clone.next_transfer_index(), 4);
    assert_eq!(cursor.next_transfer_index(), 4);
}

fn matrix(data: &[f32]) -> ConvolveMatrix {
    ConvolveMatrix {
        columns: data.len() as u32,
        rows: 1,
        target_x: 0,
        target_y: 0,
        data: data.to_vec(),
        divisor: 1.0,
        bias: 0.0,
        edge_mode: ConvolveEdgeMode::Duplicate,
        preserve_alpha: false,
    }
}

fn primitive(kind: FilterPrimitiveKind) -> FilterPrimitive {
    FilterPrimitive {
        input: FilterInput::SourceGraphic,
        input2: None,
        region: crate::shared::bounds::Bounds::canvas(17, 19),
        kind,
    }
}

#[test]
fn variable_convolve_offsets_match_the_shared_upload_blob() {
    let first = matrix(&[1.0, 2.0, 3.0]);
    let second = matrix(&[-1.0, 4.0]);
    let filter = Filter::Chain {
        fixed_region: true,
        filters: vec![
            Filter::ConvolveMatrix(first.clone()),
            Filter::Graph {
                fixed_region: true,
                primitives: vec![primitive(FilterPrimitiveKind::Filter(Box::new(
                    Filter::ConvolveMatrix(second.clone()),
                )))],
            },
        ],
    };
    let upload = FilterConvolveUpload::from_ops_and_filter(&[], &filter);
    assert_eq!(upload.kernels, [1.0, 2.0, 3.0, -1.0, 4.0]);
    let mut cursor = FilterCursors::default();
    assert_eq!(cursor.next_convolve_offset(&first), 0);
    assert_eq!(cursor.next_convolve_offset(&second), 3);
    let mut skipped = FilterCursors::default();
    skipped.advance_filter(&filter);
    assert_eq!(
        skipped.next_convolve_offset(&first) as usize,
        upload.kernels.len()
    );
}

fn turbulence(seed: i32) -> Turbulence {
    Turbulence {
        base_frequency_x: 0.1,
        base_frequency_y: 0.2,
        num_octaves: 2,
        seed,
        stitch_tiles: false,
        kind: TurbulenceKind::FractalNoise,
        linear_rgb: false,
        transform_x: 0.0,
        transform_y: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        tile_x: 0.0,
        tile_y: 0.0,
        tile_width: 17.0,
        tile_height: 19.0,
    }
}

#[test]
fn graph_turbulence_indices_match_seeded_selector_and_gradient_tables() {
    let filter = Filter::Graph {
        fixed_region: true,
        primitives: vec![
            primitive(FilterPrimitiveKind::Turbulence(turbulence(42))),
            primitive(FilterPrimitiveKind::Filter(Box::new(Filter::Graph {
                fixed_region: true,
                primitives: vec![primitive(FilterPrimitiveKind::Turbulence(turbulence(-7)))],
            }))),
        ],
    };
    let upload = FilterTurbulenceUpload::from_ops_and_filter(&[], &filter);
    assert_eq!(upload.selectors.len(), 2 * TURBULENCE_TABLE_LEN);
    assert_eq!(upload.gradients.len(), 2 * TURBULENCE_GRADIENT_LEN);
    for (index, seed) in [42, -7].into_iter().enumerate() {
        let expected = turbulence_lattice(seed);
        assert_eq!(
            &upload.selectors[index * TURBULENCE_TABLE_LEN..(index + 1) * TURBULENCE_TABLE_LEN],
            expected.selectors.as_slice()
        );
        assert_eq!(
            &upload.gradients
                [index * TURBULENCE_GRADIENT_LEN..(index + 1) * TURBULENCE_GRADIENT_LEN],
            expected.gradients.as_slice()
        );
    }
    let mut cursor = FilterCursors::default();
    cursor.advance_filter(&filter);
    assert_eq!(cursor.next_turbulence_index(), 2);
}

#[test]
fn graph_image_and_flood_brush_offsets_match_the_actual_shared_encoder() {
    let brush = Color::WHITE.into();
    let filter = Filter::Graph {
        fixed_region: true,
        primitives: vec![
            primitive(FilterPrimitiveKind::Image { brush }),
            primitive(FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                brush: Color::BLACK.into(),
            }))),
        ],
    };
    let mut canvas = Canvas::new(17, 19, 1.0);
    canvas.push_filter_layer(filter, region());
    draw(&mut canvas);
    canvas.pop_layer();
    let plan = canvas.compile(canvas.root_commands);
    let upload = GpuBrushUpload::from_filter_plan_with_resources(&plan.ops, None);
    assert!(!upload.blob.is_empty());
    let mut cursor = FilterCursors::default();
    cursor.advance_ops(&plan.ops);
    assert_eq!(
        cursor.next_brush_offset(&Color::WHITE.into()) as usize,
        upload.blob.len()
    );
}
