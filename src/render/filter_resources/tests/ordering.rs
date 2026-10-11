//! All payload kinds must share execution order across nesting, masks and localized filters.
use super::*;
use crate::shared::brush::push_encoded_brush;

fn bundle(marker: u32) -> Filter {
    Filter::Chain {
        fixed_region: true,
        filters: vec![
            transfer(marker),
            Filter::Graph {
                fixed_region: true,
                primitives: vec![
                    primitive(FilterPrimitiveKind::Filter(Box::new(
                        Filter::ConvolveMatrix(matrix(&vec![
                            marker as f32;
                            (marker % 3 + 1) as usize
                        ])),
                    ))),
                    primitive(FilterPrimitiveKind::Turbulence(turbulence(marker as i32))),
                    primitive(FilterPrimitiveKind::Image {
                        brush: Color::from_rgb8(marker as u8, 2, 3).into(),
                    }),
                    primitive(FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                        brush: Color::from_rgb8(4, marker as u8, 6).into(),
                    }))),
                ],
            },
        ],
    }
}

#[test]
fn mixed_payloads_follow_nested_backdrop_mask_and_enclosing_filter_order() {
    let mut canvas = Canvas::new(17, 19, 1.0);
    canvas.push_filter_layer(bundle(11), region());
    canvas.push_backdrop_layer(bundle(22), region());
    canvas.push_filter_layer(bundle(33), region());
    draw(&mut canvas);
    for _ in 0..3 {
        canvas.pop_layer();
    }
    let mut mask = Canvas::new(17, 19, 1.0);
    mask.push_filter_layer(bundle(55), region());
    draw(&mut mask);
    mask.pop_layer();
    canvas.push_mask_layer(
        mask,
        crate::Mask {
            region: region(),
            kind: crate::MaskKind::Alpha,
        },
    );
    canvas.push_filter_layer(bundle(44), region());
    draw(&mut canvas);
    canvas.pop_layer();
    canvas.pop_layer();
    canvas.push_filter_layer(bundle(66), region());
    draw(&mut canvas);
    canvas.pop_layer();
    let plan = canvas.compile(canvas.root_commands);
    let enclosing = bundle(77);
    for append in [false, true] {
        let upload = FilterResourceUpload::new(&plan.ops, append.then_some(&enclosing), None);
        let mut expected = vec![22, 33, 11, 44, 55, 66];
        if append {
            expected.push(77);
        }
        let markers: Vec<_> = upload
            .transfers
            .iter()
            .map(|table| {
                assert!(table.iter().all(|&value| value == table[0]));
                table[0]
            })
            .collect();
        assert_eq!(markers, expected);
        assert_eq!(upload.turbulence.len(), expected.len());
        let mut weights = Vec::new();
        let mut expected_brushes = GpuBrushUpload::default();
        for (index, &marker) in expected.iter().enumerate() {
            weights.extend(vec![marker as f32; (marker % 3 + 1) as usize]);
            let lattice = turbulence_lattice(marker as i32);
            assert_eq!(&upload.turbulence[index].selectors, &lattice.selectors);
            assert_eq!(&upload.turbulence[index].gradients, &lattice.gradients);
            for brush in [
                Color::from_rgb8(marker as u8, 2, 3),
                Color::from_rgb8(4, marker as u8, 6),
            ] {
                expected_brushes
                    .offsets
                    .push(expected_brushes.blob.len() as u32);
                push_encoded_brush(&mut expected_brushes.blob, &brush.into());
            }
        }
        assert_eq!(upload.convolves, weights);
        assert_eq!(upload.brushes.blob, expected_brushes.blob);
        assert_eq!(upload.brushes.offsets, expected_brushes.offsets);
        let mut cursor = FilterCursors::default();
        cursor.advance_ops(&plan.ops);
        if append {
            cursor.advance_filter(&enclosing);
        }
        assert_eq!(cursor.next_transfer_index() as usize, expected.len());
        assert_eq!(cursor.next_turbulence_index() as usize, expected.len());
        assert_eq!(
            cursor.next_convolve_offset(&matrix(&[1.0])) as usize,
            weights.len()
        );
        assert_eq!(
            cursor.next_brush_offset(&Color::WHITE.into()) as usize,
            upload.brushes.blob.len()
        );
    }
}

#[test]
fn empty_and_table_free_filters_prepare_no_payloads() {
    for filter in [
        None,
        Some(Filter::Opacity(0.75)),
        Some(Filter::Chain {
            fixed_region: true,
            filters: vec![],
        }),
        Some(Filter::Graph {
            fixed_region: true,
            primitives: vec![],
        }),
    ] {
        let upload = FilterResourceUpload::new(&[], filter.as_ref(), None);
        assert!(upload.transfers.is_empty());
        assert!(upload.convolves.is_empty());
        assert!(upload.turbulence.is_empty());
        assert!(upload.brushes.blob.is_empty());
        assert!(upload.brushes.offsets.is_empty());
    }
}
