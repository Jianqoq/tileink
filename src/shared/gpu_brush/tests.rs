use peniko::{Color, Extend};

use super::*;
use crate::shared::{
    bounds::PixelBounds,
    brush::{PatternBrush, PatternSampling},
    draw_record::{DrawTagWord, FillRuleWord},
    fill::FillRule,
    image_resource::ImageKey,
};

fn draw_with_brush(brush_offset: u32, brush_len: u32) -> DrawRecord {
    DrawRecord {
        path_id: DrawRecord::NONE,
        glyph_run_id: DrawRecord::NONE,
        sdf_offset: DrawRecord::NONE,
        sdf_len: 0,
        sdf_shadow_offset: DrawRecord::NONE,
        sdf_shadow_len: 0,
        brush_offset,
        brush_len,
        tag: DrawTagWord(0),
        fill_rule: FillRuleWord(FillRule::NonZero as u32),
        pixel_bounds: PixelBounds::default(),
        local_pixel_bounds: PixelBounds::default(),
        transform: crate::shared::affine::GpuAffine::IDENTITY,
        inverse_transform: crate::shared::affine::GpuAffine::IDENTITY,
        solid_rect: 0,
    }
}

#[test]
fn draw_uses_resource_brush_only_for_resource_patterns() {
    let mut blob = Vec::new();
    let solid = Brush::Solid(Color::from_rgb8(16, 32, 48));
    let (solid_offset, solid_len) = push_encoded_brush(&mut blob, &solid);
    assert!(!GpuBrushUpload::draw_uses_resource_brush(
        &draw_with_brush(solid_offset, solid_len),
        &blob
    ));

    let key = ImageKey::new(0x1234_5678);
    let resource = Brush::Pattern(
        PatternBrush::new_resource(
            ImageResourceId::renderer(key),
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            Extend::Pad,
            PatternSampling::Nearest,
            255,
        )
        .unwrap(),
    );
    let (resource_offset, resource_len) = push_encoded_brush(&mut blob, &resource);
    assert!(GpuBrushUpload::draw_uses_resource_brush(
        &draw_with_brush(resource_offset, resource_len),
        &blob
    ));
}

#[test]
fn patch_scene_brush_blob_clears_missing_resource_placement() {
    let key = ImageKey::new(0x1234_5678);
    let resource = Brush::Pattern(
        PatternBrush::new_resource(
            ImageResourceId::renderer(key),
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            Extend::Pad,
            PatternSampling::Nearest,
            255,
        )
        .unwrap(),
    );
    let mut blob = Vec::new();
    let (offset, len) = push_encoded_brush(&mut blob, &resource);
    let draw = draw_with_brush(offset, len);
    assert_eq!(blob[offset as usize], GPU_BRUSH_PATTERN_RESOURCE);

    let source = blob.clone();
    GpuBrushUpload::patch_scene_brush_blob(&mut blob, &source, &[draw], None);

    let base = offset as usize;
    assert_eq!(blob[base], GPU_BRUSH_PATTERN);
    assert_eq!(blob[base + 2], ENCODED_BRUSH_HEADER_WORDS as u32);
    assert_eq!(blob[base + 3], 0);
    assert_eq!(blob[base + 4], 0);
    assert_eq!(blob[base + 5], 0);
    assert_eq!(blob[base + 6], 0);
}

#[test]
fn resource_headers_resolve_repeated_shared_draws_and_relocated_records() {
    use crate::{Image, shared::image_resource::ImageResourceStore};
    let key = ImageKey::new(3);
    let brush = Brush::from_image_key_natural(
        key,
        [0.0; 2],
        (2, 2),
        Extend::Pad,
        PatternSampling::Nearest,
        127,
    )
    .unwrap();
    let mut source = Vec::new();
    push_encoded_brush(&mut source, &Brush::Solid(Color::BLACK));
    let (offset, len) = push_encoded_brush(&mut source, &brush);
    let draw = draw_with_brush(offset, len);
    let record = &source[offset as usize..(offset + len) as usize];
    for width in [2, 2050] {
        let mut resources = ImageResourceStore::default();
        resources.insert(
            key,
            Image::from_rgba8(width, 2, [10, 20, 30, 255].repeat(width as usize * 2)),
        );
        let upload = resources.upload_merged(&ImageResourceStore::default(), 4096, 4, 16, None);
        let mut destination = source.clone();
        // Two draws may share a record; resolving the second cannot reinterpret the first.
        GpuBrushUpload::patch_scene_brush_blob(
            &mut destination,
            &source,
            &[draw, draw],
            Some(&upload),
        );
        let expected = destination.clone();
        GpuBrushUpload::patch_scene_brush_blob_draw_ranges(
            &mut destination,
            &source,
            &[draw],
            std::slice::from_ref(&(0..1)),
            Some(&upload),
        );
        assert_eq!(destination, expected);
        let header = &destination[offset as usize..offset as usize + GPU_BRUSH_U32_STRIDE];
        assert_eq!(header[0], GPU_BRUSH_PATTERN_RESOURCE);
        assert_eq!((header[5], header[6], header[7]), (width, 2, 127));
        assert_eq!(
            header[4] & GPU_RESOURCE_TEXTURE_PLACEMENT_BIT != 0,
            width == 2050
        );

        // Shader-brush packing relocates ordinary image records into a different blob.
        let mut relocated = record.to_vec();
        GpuBrushUpload::patch_brush_record(&mut relocated, record, Some(&upload));
        assert_eq!(
            relocated,
            destination[offset as usize..(offset + len) as usize]
        );
        assert_eq!(
            &relocated[GPU_BRUSH_U32_STRIDE..],
            &record[GPU_BRUSH_U32_STRIDE..]
        );

        // A missing image clears the header, and a later insertion resolves from the source ID.
        GpuBrushUpload::patch_brush_record(&mut relocated, record, None);
        assert_eq!(relocated[0], GPU_BRUSH_PATTERN);
        GpuBrushUpload::patch_brush_record(&mut relocated, record, Some(&upload));
        assert_eq!(
            relocated,
            destination[offset as usize..(offset + len) as usize]
        );
    }
}
