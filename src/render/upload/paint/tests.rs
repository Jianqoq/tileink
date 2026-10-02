use super::*;
use crate::{
    Image, Radius,
    canvas::SceneBufferChanges,
    shared::{
        brush::{Brush, PatternBrush, PatternSampling},
        image_resource::{ImageKey, ImageResourceId, ImageResourceStore},
    },
};
use peniko::{Color, Extend, kurbo::Rect};

fn canvas_words() -> Canvas {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.sdf_blob = vec![10, 11, 12, 13];
    canvas.sdf_shadow_blob = vec![20, 21];
    canvas.brush_blob = vec![30, 31, 32];
    canvas
}

fn retained_data(upload: PreparedPaint<'_>) -> (&[u32], Vec<std::ops::Range<usize>>, u32, u32) {
    let PaintData::Retained { words, ranges } = upload.data else {
        panic!("retained input must produce a range upload")
    };
    (words, ranges, upload.shadow_base, upload.brush_base)
}

#[test]
fn immediate_upload_borrows_the_three_original_slices() {
    let canvas = canvas_words();
    let mut state = PaintUploadState::default();
    let upload = state.prepare(&canvas, None);
    assert_eq!((upload.shadow_base, upload.brush_base), (4, 6));
    let PaintData::Immediate {
        sdfs,
        shadows,
        brushes,
    } = upload.data
    else {
        panic!("immediate input must retain the direct-slice path")
    };
    assert!(std::ptr::eq(sdfs, canvas.sdf_blob.as_slice()));
    assert!(std::ptr::eq(shadows, canvas.sdf_shadow_blob.as_slice()));
    assert!(std::ptr::eq(brushes, canvas.brush_blob.as_slice()));
}

#[test]
fn retained_partial_ranges_keep_offsets_and_unmodified_words() {
    let mut canvas = canvas_words();
    canvas.buffer_changes = Some(SceneBufferChanges::default());
    let mut state = PaintUploadState::default();
    let (words, ranges, shadow, brush) = retained_data(state.prepare(&canvas, None));
    assert_eq!((shadow, brush), (256, 512));
    assert_eq!(ranges, std::iter::once(0..768).collect::<Vec<_>>());
    assert_eq!(&words[..4], &[10, 11, 12, 13]);
    assert!(words[4..256].iter().all(|word| *word == 0));
    assert_eq!(&words[256..258], &[20, 21]);
    assert_eq!(&words[512..515], &[30, 31, 32]);

    canvas.sdf_blob[2] = 112;
    canvas.sdf_shadow_blob[1] = 121;
    canvas.brush_blob[0] = 130;
    canvas.buffer_changes = Some(SceneBufferChanges {
        sdfs: std::iter::once(2..3).collect::<Vec<_>>(),
        shadows: std::iter::once(1..2).collect::<Vec<_>>(),
        brushes: std::iter::once(0..1).collect::<Vec<_>>(),
        ..Default::default()
    });
    let (words, ranges, next_shadow, next_brush) = retained_data(state.prepare(&canvas, None));
    assert_eq!((next_shadow, next_brush), (shadow, brush));
    assert_eq!(ranges, vec![2..3, 257..258, 512..513]);
    assert_eq!(&words[..4], &[10, 11, 112, 13]);
    assert_eq!(&words[256..258], &[20, 121]);
    assert_eq!(&words[512..515], &[130, 31, 32]);
    canvas.buffer_changes = Some(SceneBufferChanges::default());
    let (_, ranges, _, _) = retained_data(state.prepare(&canvas, None));
    assert!(ranges.is_empty());
}

#[test]
fn growth_rebases_and_uploads_all_segments_including_clean_ones() {
    let mut canvas = canvas_words();
    canvas.buffer_changes = Some(SceneBufferChanges::default());
    let mut state = PaintUploadState::default();
    let _ = state.prepare(&canvas, None);
    canvas.sdf_blob.resize(257, 99);
    canvas.buffer_changes = Some(SceneBufferChanges {
        sdfs: std::iter::once(4..257).collect::<Vec<_>>(),
        ..Default::default()
    });
    let (words, ranges, shadow, brush) = retained_data(state.prepare(&canvas, None));
    assert_eq!((shadow, brush), (512, 768));
    assert_eq!(ranges, std::iter::once(0..1024).collect::<Vec<_>>());
    assert_eq!(&words[..257], &canvas.sdf_blob);
    assert_eq!(&words[512..514], &[20, 21]);
    assert_eq!(&words[768..771], &[30, 31, 32]);
}

#[test]
fn switching_through_immediate_upload_rebuilds_retained_storage() {
    let mut canvas = canvas_words();
    canvas.buffer_changes = Some(SceneBufferChanges::default());
    let mut state = PaintUploadState::default();
    let _ = state.prepare(&canvas, None);
    let immediate = Canvas::new(32, 32, 1.0);
    assert!(matches!(
        state.prepare(&immediate, None).data,
        PaintData::Immediate { .. }
    ));
    let (words, ranges, _, _) = retained_data(state.prepare(&canvas, None));
    assert_eq!(ranges, std::iter::once(0..768).collect::<Vec<_>>());
    assert_eq!(&words[..4], &canvas.sdf_blob);
}

fn resource_brush(key: ImageKey) -> Brush {
    Brush::Pattern(
        PatternBrush::new_resource(
            ImageResourceId::renderer(key),
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            Extend::Pad,
            PatternSampling::Nearest,
            255,
        )
        .unwrap(),
    )
}

fn resources(key: ImageKey) -> ImageResourceStore {
    let mut resources = ImageResourceStore::default();
    resources.insert(key, Image::from_rgba8(2, 2, [10, 20, 30, 255].repeat(4)));
    resources
}

fn expected_brushes(canvas: &Canvas, resources: &GpuImageResourceUpload) -> Vec<u32> {
    let mut expected = canvas.brush_blob.clone();
    GpuBrushUpload::patch_scene_brush_blob(&mut expected, &canvas.draw_records, Some(resources));
    expected
}

#[test]
fn image_placement_changes_repatch_brushes_without_a_scene_mutation() {
    let key = ImageKey::new(2);
    let mut resources = resources(key);
    // The second image moves from page 1 to page 0 when the atlas grows.
    // A page-size-only change for one image leaves its encoded placement equal.
    resources.insert(
        ImageKey::new(1),
        Image::from_rgba8(2, 2, [40, 50, 60, 255].repeat(4)),
    );
    let empty = ImageResourceStore::default();
    let first = resources.upload_merged(&empty, 4, 4, 0, None);
    let second = resources.upload_merged(&empty, 8, 4, 0, Some(&first));
    assert_ne!(first.generation(), second.generation());
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 20.0, 20.0),
        Radius::ZERO,
        resource_brush(key),
    );
    canvas.buffer_changes = Some(SceneBufferChanges::default());
    let mut state = PaintUploadState::default();
    let (words, _, _, base) = retained_data(state.prepare(&canvas, Some(&first)));
    let before = words[base as usize..base as usize + canvas.brush_blob.len()].to_vec();
    let (words, ranges, _, base) = retained_data(state.prepare(&canvas, Some(&second)));
    let expected = expected_brushes(&canvas, &second);
    assert_ne!(before, expected);
    assert_eq!(
        &words[base as usize..base as usize + expected.len()],
        &expected
    );
    assert_eq!(ranges, vec![base as usize..base as usize + expected.len()]);
}

#[test]
fn resource_membership_is_rebuilt_after_mutations_with_no_image_table() {
    let key = ImageKey::new(2);
    let mut canvas = Canvas::new(32, 32, 1.0);
    let draw = canvas.push_rect(Rect::new(0.0, 0.0, 20.0, 20.0), Radius::ZERO, Color::BLACK);
    canvas.buffer_changes = Some(SceneBufferChanges::default());
    let mut state = PaintUploadState::default();
    let _ = state.prepare(&canvas, None);
    assert!(canvas.set_draw_brush(draw, resource_brush(key)));
    canvas.buffer_changes = Some(SceneBufferChanges {
        draws: std::iter::once(0..1).collect::<Vec<_>>(),
        brushes: std::iter::once(0..canvas.brush_blob.len()).collect::<Vec<_>>(),
        ..Default::default()
    });
    let _ = state.prepare(&canvas, None);
    canvas.buffer_changes = Some(SceneBufferChanges::default());
    let upload = resources(key).upload_merged(&ImageResourceStore::default(), 8, 4, 0, None);
    let (words, _, _, base) = retained_data(state.prepare(&canvas, Some(&upload)));
    let expected = expected_brushes(&canvas, &upload);
    assert_eq!(
        &words[base as usize..base as usize + expected.len()],
        &expected
    );
}

#[test]
fn paint_layout_keeps_segment_bases_stable_until_capacity_is_exhausted() {
    let initial = grow_paint_layout((0, 0, 0), (100, 20, 200));
    assert_eq!(initial, (256, 256, 256));
    assert_eq!(grow_paint_layout(initial, (120, 10, 250)), initial);
    assert_eq!(grow_paint_layout(initial, (257, 10, 250)), (512, 256, 256));
    assert_eq!(grow_paint_layout((0, 0, 0), (256, 0, 0)).0, 512);
    assert_eq!(
        grow_paint_layout((1024, 512, 256), (100, 0, 200)),
        (256, 0, 256)
    );
}
