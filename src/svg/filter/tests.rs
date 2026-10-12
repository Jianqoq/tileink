use super::*;
use crate::SvgOptions;
use peniko::kurbo::Rect;
use usvg::Node;

fn with_filter<T>(
    body: &str,
    transform: Affine,
    f: impl FnOnce(SvgFilterGraphBuilder<'_>, &usvg::filter::Filter) -> T,
) -> T {
    let tree = usvg::Tree::from_str(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><defs><filter id="f" filterUnits="userSpaceOnUse" primitiveUnits="userSpaceOnUse" x="0" y="0" width="100" height="80">{body}</filter></defs><rect width="100" height="80" filter="url(#f)"/></svg>"#), &usvg::Options::default()).unwrap();
    let Node::Group(group) = &tree.root().children()[0] else {
        panic!("filtered group");
    };
    let filter = &group.filters()[0];
    let builder = SvgBuilder::new(SvgOptions::default());
    let mut canvas = Canvas::new(100, 80, 1.0);
    let graph = SvgFilterGraphBuilder::new(
        &builder,
        &mut canvas,
        transform,
        transform,
        Bounds::canvas(100, 80),
    );
    f(graph, filter)
}

#[test]
fn merge_and_tile_preserve_union_and_empty_input_domains() {
    for (body, expected) in [
        (
            r#"<feFlood x="10" y="12" width="20" height="18" result="a"/><feFlood x="35" y="20" width="8" height="7" result="b"/><feMerge result="merged"><feMergeNode in="a"/><feMergeNode in="b"/></feMerge><feTile in="merged"/>"#,
            Bounds::new(10, 12, 43, 30),
        ),
        (
            r#"<feFlood x="110" y="0" width="10" height="10" result="a"/><feMerge result="merged"><feMergeNode in="a"/></feMerge><feTile in="merged"/>"#,
            Bounds::new(0, 0, 0, 0),
        ),
    ] {
        with_filter(body, Affine::IDENTITY, |mut graph, filter| {
            for primitive in filter.primitives() {
                graph.push_primitive(primitive).unwrap();
            }
            let FilterPrimitiveKind::Tile { source_region } = graph.primitives.last().unwrap().kind
            else {
                panic!("tile");
            };
            if expected.is_empty() {
                assert!(source_region.is_empty());
                assert!(graph.source_regions.last().unwrap().is_empty());
            } else {
                assert_eq!(source_region, expected);
                assert_eq!(
                    *graph.source_regions.last().unwrap(),
                    Bounds::canvas(100, 80)
                );
            }
        });
    }
}

#[test]
fn blur_and_dilation_track_transformed_source_domains() {
    with_filter(
        r#"<feFlood x="10" y="10" width="5" height="5"/><feGaussianBlur stdDeviation="1 2"/><feMorphology operator="dilate" radius="1 2"/>"#,
        Affine::scale_non_uniform(2.0, 1.0),
        |mut graph, filter| {
            for primitive in filter.primitives() {
                graph.push_primitive(primitive).unwrap();
            }
            assert_eq!(
                graph.source_regions,
                [
                    Bounds::new(20, 10, 30, 15),
                    Bounds::new(14, 4, 36, 21),
                    Bounds::new(12, 2, 38, 23)
                ]
            );
        },
    );
}

#[test]
fn filter_regions_keep_affine_rectangle_bounds() {
    let transform = Affine::new([1.0, 0.25, -0.5, 1.0, 3.0, -4.0]);
    with_filter("<feFlood/>", transform, |graph, filter| {
        let layer = svg_filter_layer(
            graph.builder,
            graph.canvas,
            filter,
            transform,
            transform,
            100,
            80,
        )
        .unwrap()
        .unwrap();
        let Region::Rect { rect, radius } = layer.region else {
            panic!("rectangular filter region");
        };
        assert_eq!(
            rect,
            transform.transform_rect_bbox(Rect::new(0.0, 0.0, 100.0, 80.0))
        );
        assert_eq!(
            [
                radius.top_left,
                radius.top_right,
                radius.bottom_right,
                radius.bottom_left
            ],
            [0.0; 4]
        );
    });
}
