//! Retained scene construction and mutation shared by GPU runs and CPU tests.

use super::super::Result;
use super::Frame;
#[cfg(test)]
use super::{FRAMES, names};
#[cfg(windows)]
use peniko::kurbo::Point;
use peniko::{
    Color, Extend,
    kurbo::{Affine, Circle, Rect, Shape},
};
use std::rc::Rc;
use tileink::{
    Canvas, FillRule, Filter, Image, Mask, MaskKind, PatternSampling, Radius, Region,
    RetainedLayerDescriptor as Layer, RetainedNodeId as Node, RetainedParent as Parent,
    RetainedScene,
};
#[cfg(windows)]
use tileink::{TextContext, TextLayoutOptions};

pub struct Sequence {
    pub scene: RetainedScene,
    pub background: [u8; 4],
    labels: [Rc<Canvas>; 3],
}

fn node(index: u64) -> Node {
    Node::for_owner(970_000 + index)
}
fn parent(index: u64) -> Parent {
    Parent::content(node(index))
}
fn solid(scale: f32, rect: Rect, color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(256, 160, scale);
    canvas.push_rect(rect, Radius::ZERO, color);
    Rc::new(canvas)
}
fn geometry(scale: f32, value: u8, shift: f64) -> Rc<Canvas> {
    let mut canvas = Canvas::new(256, 160, scale);
    canvas.push_rect(
        Rect::new(4.25 + shift, 8.5, 41.75 + shift, 40.125),
        Radius::all(5.25),
        Color::from_rgba8(value, 72, 190, 183),
    );
    canvas.push_path(
        Circle::new((39.25 + shift, 30.5), 17.125).to_path(0.1),
        Color::from_rgba8(23, 179, 107, 221),
        Affine::IDENTITY,
        FillRule::EvenOdd,
        0.1,
    );
    Rc::new(canvas)
}
fn image(scale: f32, value: u8) -> Rc<Canvas> {
    let mut canvas = Canvas::new(256, 160, scale);
    canvas.push_image(
        Rect::new(17.25, 38.125, 81.75, 71.5),
        Image::from_rgba8(
            2,
            2,
            [
                value, 15, 201, 192, 37, 201, 94, 128, 147, 223, 14, 255, 43, 20, 173, 0,
            ],
        ),
        Extend::Pad,
        PatternSampling::Bilinear,
    );
    Rc::new(canvas)
}
fn region() -> Region {
    Region::rect(Rect::new(33.25, 9.5, 119.75, 73.25), Radius::all(4.0))
}

impl Sequence {
    // Font-backed capture is used by the Windows four-route GPU runner.
    // Keep the frame-state model and its CPU tests available on other targets.
    #[cfg(windows)]
    pub fn new(fonts: &super::super::common::fonts::Snapshot) -> Result<Self> {
        let mut font_system = fonts.font_system();
        let mut text_context = TextContext::new();
        let mut label = |text: &str, x: f64, scale: f32| -> Result<Rc<Canvas>> {
            let layout = text_context.layout(&mut font_system, TextLayoutOptions::new(text, 18.0));
            if layout.is_empty() {
                return Err("retained text input produced no glyphs".into());
            }
            let mut canvas = Canvas::new(256, 160, scale);
            canvas.push_text_layout_clipped(
                &layout,
                Point::new(x, 76.0),
                Rect::new(4.0, 51.0, 105.5, 79.0),
                Color::from_rgba8(229, 233, 239, 213),
            );
            Ok(Rc::new(canvas))
        };
        Self::from_labels([
            label("Tile 17", 3.25, 1.0)?,
            label("Tile 81", 11.75, 1.0)?,
            label("Tile 81", 11.75, 1.25)?,
        ])
    }

    fn from_labels(labels: [Rc<Canvas>; 3]) -> Result<Self> {
        let scale = 1.0;
        let mut scene = RetainedScene::new(257, 161, 1.0, node(0))?;
        scene
            .transaction()
            .insert_scene(
                parent(0),
                None,
                node(1),
                solid(
                    scale,
                    Rect::new(0.0, 0.0, 256.0, 160.0),
                    Color::from_rgb8(20, 30, 40),
                ),
                Affine::IDENTITY,
            )
            .insert_scene(
                parent(0),
                None,
                node(2),
                geometry(scale, 201, 0.0),
                Affine::IDENTITY,
            )
            .insert_scene(parent(0), None, node(3), image(scale, 83), Affine::IDENTITY)
            .insert_layer(
                parent(0),
                None,
                node(4),
                Layer::ClipPath {
                    path: Rect::new(18.5, 6.25, 103.75, 62.5).to_path(0.1),
                    transform: Affine::IDENTITY,
                    rule: FillRule::NonZero,
                    tolerance: 0.1,
                },
            )
            .insert_layer(
                parent(0),
                None,
                node(5),
                Layer::Mask(Mask {
                    region: region(),
                    kind: MaskKind::Alpha,
                }),
            )
            .insert_scene(
                parent(5),
                None,
                node(6),
                solid(
                    scale,
                    Rect::new(71.5, 13.25, 121.5, 48.75),
                    Color::from_rgba8(209, 52, 38, 197),
                ),
                Affine::IDENTITY,
            )
            .insert_scene(
                Parent::mask(node(5)),
                None,
                node(7),
                solid(
                    scale,
                    Rect::new(83.0, 17.0, 117.0, 42.0),
                    Color::from_rgba8(255, 255, 255, 163),
                ),
                Affine::IDENTITY,
            )
            .insert_layer(
                parent(0),
                None,
                node(8),
                Layer::Backdrop {
                    filter: Filter::Blur {
                        std_dev_x: 2.25,
                        std_dev_y: 1.75,
                        sampling: Default::default(),
                    },
                    sample_region: region(),
                },
            )
            .insert_scene(
                parent(0),
                None,
                node(9),
                labels[0].clone(),
                Affine::IDENTITY,
            )
            .commit()?;
        Ok(Self {
            scene,
            labels,
            background: [20, 30, 40, 255],
        })
    }

    pub fn apply(&mut self, frame: Frame) -> Result<()> {
        use Frame::*;
        let scale = self.scene.scale_factor();
        match frame {
            Geometry => {
                self.scene
                    .transaction()
                    .replace_scene(node(2), geometry(scale, 217, 2.5))
                    .commit()?;
            }
            Image => {
                self.scene
                    .transaction()
                    .replace_scene(node(3), image(scale, 249))
                    .commit()?;
            }
            Translate => {
                self.scene
                    .transaction()
                    .set_transform(node(2), Affine::translate((-1.25, 4.75)))
                    .commit()?;
            }
            Reorder => {
                self.scene
                    .transaction()
                    .move_before(node(3), node(2))
                    .commit()?;
            }
            Reparent => {
                self.scene
                    .transaction()
                    .reparent(node(2), parent(4), None)
                    .commit()?;
            }
            Mask => {
                self.scene
                    .transaction()
                    .replace_scene(
                        node(7),
                        solid(
                            scale,
                            Rect::new(74.5, 15.25, 112.25, 46.75),
                            Color::from_rgba8(255, 255, 255, 211),
                        ),
                    )
                    .commit()?;
            }
            Background => {
                self.background = [30, 55, 91, 255];
                self.scene
                    .transaction()
                    .replace_scene(
                        node(1),
                        solid(
                            scale,
                            Rect::new(0.0, 0.0, 256.0, 160.0),
                            Color::from_rgb8(30, 55, 91),
                        ),
                    )
                    .commit()?;
            }
            Text => {
                self.scene
                    .transaction()
                    .replace_scene(node(9), self.labels[1].clone())
                    .commit()?;
            }
            Remove => {
                self.scene.transaction().remove_subtree(node(2)).commit()?;
            }
            Reinsert => {
                self.scene
                    .transaction()
                    .insert_scene(
                        parent(0),
                        Some(node(3)),
                        node(2),
                        geometry(scale, 167, 1.25),
                        Affine::IDENTITY,
                    )
                    .commit()?;
            }
            Invalidate => {
                self.scene
                    .transaction()
                    .invalidate_rect(Rect::new(1.5, 2.5, 33.25, 34.75))
                    .commit()?;
            }
            Grow | Shrink | Tile15 | Tile16 | Tile17 => {
                let (width, height, dpi) = match frame {
                    Grow => (321, 193, 1.0),
                    Shrink => (97, 65, 1.0),
                    Tile15 => (15, 17, 1.0),
                    Tile16 => (16, 15, 1.0),
                    Tile17 => (17, 16, 1.0),
                    _ => unreachable!(),
                };
                self.scene
                    .transaction()
                    .resize(width, height, dpi)
                    .commit()?;
            }
            Dpi => {
                // Canvas primitives are built at their DPI. Keep the scene and layer
                // identities, but atomically replace leaves around the scale change;
                // resizing while old-scale canvases are attached is invalid.
                let scale = 1.25;
                let mut tx = self.scene.transaction();
                for index in [1, 2, 3, 6, 7, 9] {
                    tx.remove_subtree(node(index));
                }
                tx.resize(257, 161, scale)
                    .insert_scene(
                        parent(0),
                        Some(node(4)),
                        node(1),
                        solid(
                            scale,
                            Rect::new(0.0, 0.0, 256.0, 160.0),
                            Color::from_rgb8(30, 55, 91),
                        ),
                        Affine::IDENTITY,
                    )
                    .insert_scene(
                        parent(0),
                        Some(node(4)),
                        node(2),
                        geometry(scale, 167, 1.25),
                        Affine::IDENTITY,
                    )
                    .insert_scene(
                        parent(0),
                        Some(node(4)),
                        node(3),
                        image(scale, 249),
                        Affine::IDENTITY,
                    )
                    .insert_scene(
                        parent(5),
                        None,
                        node(6),
                        solid(
                            scale,
                            Rect::new(71.5, 13.25, 121.5, 48.75),
                            Color::from_rgba8(209, 52, 38, 197),
                        ),
                        Affine::IDENTITY,
                    )
                    .insert_scene(
                        Parent::mask(node(5)),
                        None,
                        node(7),
                        solid(
                            scale,
                            Rect::new(74.5, 15.25, 112.25, 46.75),
                            Color::from_rgba8(255, 255, 255, 211),
                        ),
                        Affine::IDENTITY,
                    )
                    .insert_scene(
                        parent(0),
                        None,
                        node(9),
                        self.labels[2].clone(),
                        Affine::IDENTITY,
                    )
                    .commit()?;
            }
            AfterResize => {
                self.scene
                    .transaction()
                    .set_transform(node(2), Affine::translate((1.25, -2.5)))
                    .commit()?;
            }
            SwapImage => {
                // Image 0 now contains an older scene. Returning to its unchanged
                // history ID must catch up after image 1 consumed this commit.
                self.scene
                    .transaction()
                    .replace_scene(node(2), geometry(scale, 47, 4.75))
                    .commit()?;
            }
            JournalGap => {
                // More than the retained journal capacity, with no renderer consuming it.
                // A correct full resynchronization must still resume incremental updates.
                for version in 0..257u16 {
                    self.scene
                        .transaction()
                        .replace_scene(node(2), geometry(scale, (version % 251) as u8 + 1, 1.25))
                        .commit()?;
                }
            }
            Resume => {
                self.scene
                    .transaction()
                    .replace_scene(node(2), geometry(scale, 231, 1.25))
                    .commit()?;
            }
            Empty => {
                let mut tx = self.scene.transaction();
                for index in [1, 2, 3, 4, 5, 8, 9] {
                    tx.remove_subtree(node(index));
                }
                tx.commit()?;
                self.background = [0; 4];
            }
            Initial | Static | ReplaceTarget | FreshHistory | ExternalClear | ReturnImage
            | EmptyStatic => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_manifest_is_unique_and_all_transactions_are_valid() -> Result<()> {
        let names = names();
        assert_eq!(names.len(), 29);
        assert_eq!(
            names
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            names.len()
        );
        let label = |scale| solid(scale, Rect::new(4.0, 61.0, 31.0, 76.0), Color::WHITE);
        let mut sequence = Sequence::from_labels([label(1.0), label(1.0), label(1.25)])?;
        for &frame in FRAMES {
            let before = sequence.scene.version();
            sequence
                .apply(frame)
                .map_err(|error| format!("{}: {error}", frame.name()))?;
            if frame == Frame::Dpi {
                assert_eq!(sequence.scene.version().get() - before.get(), 1);
                assert_eq!(sequence.scene.scale_factor(), 1.25);
                assert_eq!(sequence.scene.physical_size(), (322, 202));
            }
            if frame == Frame::JournalGap {
                assert_eq!(sequence.scene.version().get() - before.get(), 257);
            }
            if frame == Frame::SwapImage {
                assert!(sequence.scene.version() > before);
            }
            if frame == Frame::ReturnImage {
                assert_eq!(sequence.scene.version(), before);
            }
            assert!(sequence.scene.physical_size().0 > 0);
        }
        assert_eq!(sequence.background, [0; 4]);
        assert_eq!(sequence.scene.physical_size(), (322, 202));
        Ok(())
    }
}
