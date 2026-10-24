//! Isolating a fading form must not increase the label's coverage or stroke contrast.
#![cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
use std::rc::Rc;

use peniko::{
    Color,
    kurbo::{Affine, Point, Rect, Shape},
};
use tileink::{
    Canvas, NativeBackend, NativeRenderer, Radius, RetainedLayerDescriptor, RetainedNodeId,
    RetainedParent, RetainedScene, TextAttrs, TextCompositeMode, TextContext, TextFamily,
    TextFontSystem, TextLayoutOptions, TextRasterOptions, TextSubpixelMode, TextWeight,
};

fn ink(pixels: &[u32]) -> (u64, u16) {
    let background = pixels[0].to_le_bytes();
    pixels.iter().fold((0, 0), |(mass, peak), pixel| {
        let contrast = pixel.to_le_bytes()[..3]
            .iter()
            .zip(background)
            .map(|(&value, background)| u16::from(value.abs_diff(background)))
            .sum::<u16>();
        (mass + u64::from(contrast), peak.max(contrast))
    })
}

fn verify_fade(
    options: TextRasterOptions,
    scale: u32,
    dark: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    let mut renderer = NativeRenderer::new(backend, 160 * scale, 64 * scale)?;
    let mut fonts = TextFontSystem::new();
    fonts.db_mut().load_font_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/svg/fonts/NotoSans-Regular.ttf"
    ))?;
    let mut text = TextContext::new();
    text.set_raster_options(options);
    let layout = text.layout(
        &mut fonts,
        TextLayoutOptions::new("Provider", 11.0).with_attrs(
            TextAttrs::new()
                .family(TextFamily::Name("Noto Sans"))
                .weight(TextWeight::MEDIUM),
        ),
    );
    let bounds = Rect::new(0.0, 0.0, 160.0, 64.0);
    let (background_color, foreground) = if dark {
        (
            Color::from_rgb8(24, 28, 36),
            Color::from_rgb8(220, 224, 232),
        )
    } else {
        (Color::WHITE, Color::from_rgb8(105, 116, 135))
    };
    let mut background = Canvas::new(160, 64, scale as f32);
    background.push_rect(bounds, Radius::ZERO, background_color);
    let mut label = Canvas::new(160, 64, scale as f32);
    // The control and clipped label occupy different draw stacks, so the opacity group
    // must be isolated. Its real backdrop belongs outside the fading subtree.
    label.push_rect(
        Rect::new(16.0, 40.0, 144.0, 60.0),
        Radius::ZERO,
        background_color,
    );
    label.push_clip_sdf_rect_layer(bounds, Radius::ZERO);
    label.push_text_layout(&layout, Point::new(16.0, 16.0), foreground);
    label.pop_layer();
    let background = Rc::new(background);
    let label = Rc::new(label);
    let root = RetainedNodeId::for_owner(1);
    let layer = RetainedNodeId::for_owner(2);
    let mut scene = RetainedScene::new(160, 64, scale as f32, root)?;
    let descriptor = |opacity| RetainedLayerDescriptor::Opacity {
        path: bounds.to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        opacity,
    };
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(3),
            Rc::clone(&background),
            Affine::IDENTITY,
        )
        .insert_layer(RetainedParent::content(root), None, layer, descriptor(1.0))
        .insert_scene(
            RetainedParent::content(layer),
            None,
            RetainedNodeId::for_owner(4),
            Rc::clone(&label),
            Affine::IDENTITY,
        )
        .commit()?;
    let mut ungrouped = RetainedScene::new(160, 64, scale as f32, root)?;
    ungrouped
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(3),
            background,
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(4),
            label,
            Affine::IDENTITY,
        )
        .commit()?;
    // At rest gfx_ui removes its opacity scope; a reference that already has an
    // offscreen layer reproduces the same bug and cannot catch its insertion.
    let reference = renderer
        .render_retained_to_image_with_text(&ungrouped, &mut fonts, &mut text)?
        .readback()?;
    let normal = ink(&reference.pixels);
    assert!(normal.0 > 0);
    for alpha in [1.0, 0.95, 0.5, 0.05, 0.0, 1.0] {
        for offset in [0.0, 0.125, 0.5, 1.0] {
            scene
                .transaction()
                .update_layer(layer, descriptor(alpha))
                .set_transform(
                    RetainedNodeId::for_owner(4),
                    Affine::translate((0.0, offset)),
                )
                .commit()?;
            let image = renderer
                .render_retained_to_image_with_text(&scene, &mut fonts, &mut text)?
                .readback()?;
            let current = ink(&image.pixels);
            let correct = current.0 * 100 <= normal.0 * 110
                && u32::from(current.1) * 100 <= u32::from(normal.1) * 110;
            if !correct {
                reference.save("target/text-fade-reference.png")?;
                image.save("target/text-fade-actual.png")?;
            }
            assert!(
                correct,
                "{options:?}, scale={scale}, dark={dark}, opacity={alpha}, offset={offset}: ink and peak {current:?} exceed settled text {normal:?}"
            );
            if alpha == 0.0 {
                assert_eq!(current.0, 0, "zero opacity must leave no text");
            } else {
                assert!(current.0 > 0, "nonzero opacity must retain the label");
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires a native GPU"]
fn fading_clipped_label_does_not_gain_ink() -> Result<(), Box<dyn std::error::Error>> {
    for subpixel in [TextSubpixelMode::Rgb, TextSubpixelMode::Bgr] {
        for composite in [TextCompositeMode::Linear, TextCompositeMode::Srgb] {
            for scale in [1, 2] {
                for dark in [false, true] {
                    verify_fade(
                        TextRasterOptions::new()
                            .with_subpixel_mode(subpixel)
                            .with_composite_mode(composite),
                        scale,
                        dark,
                    )?;
                }
            }
        }
    }
    Ok(())
}
