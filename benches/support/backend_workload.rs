use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::rc::Rc;
use tileink::{Canvas, Radius, RetainedNodeId, RetainedParent, RetainedScene};

pub const CASES: [&str; 11] = [
    "unchanged",
    "sparse",
    "full",
    "resize",
    "blur",
    "text",
    "images",
    "image_replace",
    "clips",
    "paths",
    "large",
];

pub struct Workload {
    pub(super) text: Option<(tileink::TextFontSystem, tileink::TextContext)>,
    pub scene: RetainedScene,
    name: &'static str,
    phase: u32,
    image_revision: u64,
}

impl Workload {
    pub fn new(name: &'static str) -> Self {
        let root = RetainedNodeId::for_owner(1);
        let mut scene = RetainedScene::new(1280, 800, 1.0, root).unwrap();
        let mut text = (name == "text").then(|| {
            let mut database = cosmic_text::fontdb::Database::new();
            database.load_font_data(
                include_bytes!("../../src/svg/fonts/NotoSans-Regular.ttf").to_vec(),
            );
            database.set_sans_serif_family("Noto Sans");
            (
                tileink::TextFontSystem::new_with_locale_and_db("en-US".into(), database),
                tileink::TextContext::new(),
            )
        });
        let mut transaction = scene.transaction();
        for index in 0..Self::count(name) {
            let mut canvas = Canvas::new(1280, 800, 1.0);
            let rect = Rect::new(0.25, 0.5, 65.75, 23.25);
            if name == "blur" && index < 8 {
                canvas.push_filter_layer(
                    tileink::Filter::Blur {
                        std_dev_x: 2.0,
                        std_dev_y: 2.0,
                        sampling: Default::default(),
                    },
                    tileink::Region::rect(Rect::new(0.0, 0.0, 72.0, 30.0), Radius::ZERO),
                );
            }
            let color = Color::from_rgba8((40 + index % 160) as u8, 110, 190, 211);
            match name {
                "text" => {
                    let (fonts, context) = text.as_mut().unwrap();
                    let layout =
                        context.layout(fonts, tileink::TextLayoutOptions::new("Chart 123", 12.0));
                    canvas.push_text_layout(&layout, peniko::kurbo::Point::new(0.25, 0.5), color);
                }
                "images" | "image_replace" => {
                    canvas = image_canvas(if name == "image_replace" {
                        replacement_image(index, 0)
                    } else {
                        image(index)
                    });
                }
                "clips" => {
                    use peniko::kurbo::Shape;
                    canvas.push_clip_layer(
                        peniko::kurbo::Circle::new((30.0, 12.0), 18.0).to_path(0.1),
                        Affine::IDENTITY,
                        tileink::FillRule::NonZero,
                        0.1,
                    );
                    canvas.push_rect(rect, Radius::ZERO, color);
                    canvas.pop_layer();
                }
                "paths" => {
                    let mut path = peniko::kurbo::BezPath::new();
                    path.move_to((0.25, 12.5));
                    for part in 0..8 {
                        let x = f64::from(part) * 8.0;
                        path.curve_to((x + 2.0, -8.0), (x + 6.0, 32.0), (x + 8.0, 12.5));
                    }
                    path.line_to((64.25, 24.0));
                    path.line_to((0.25, 24.0));
                    path.close_path();
                    canvas.push_path(
                        path,
                        color,
                        Affine::IDENTITY,
                        tileink::FillRule::NonZero,
                        0.1,
                    );
                }
                "large" => {
                    canvas.push_rect(Rect::new(0.25, 0.5, 16.75, 8.25), Radius::ZERO, color);
                }
                _ => {
                    canvas.push_rect(rect, Radius::ZERO, color);
                }
            }
            if name == "blur" && index < 8 {
                canvas.pop_layer();
            }
            transaction.insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(index + 2),
                Rc::new(canvas),
                Self::transform(name, index, 0),
            );
        }
        transaction.commit().unwrap();
        Self {
            scene,
            text,
            name,
            phase: 0,
            image_revision: 0,
        }
    }

    fn count(name: &str) -> u64 {
        if name == "large" { 4096 } else { 384 }
    }

    fn transform(name: &str, index: u64, phase: u32) -> Affine {
        let (columns, dx, dy) = if name == "large" {
            (64, 20.0, 12.0)
        } else {
            (16, 76.0, 32.0)
        };
        Affine::translate((
            (index % columns) as f64 * dx + 8.0 + f64::from(phase % 2),
            (index / columns) as f64 * dy + 8.0,
        ))
    }

    pub fn size(&self) -> [u32; 2] {
        let step = if self.name == "resize" {
            self.phase.min(16 - self.phase)
        } else {
            0
        };
        [1280 - step * 8, 800 - step * 5]
    }

    pub fn advance(&mut self) {
        self.phase = (self.phase + 1) % 16;
        if self.name == "unchanged" {
            return;
        }
        // Replacement content never cycles back into a warm image cache.
        if self.name == "image_replace" {
            self.image_revision = self.image_revision.checked_add(1).unwrap();
        }
        let mut transaction = self.scene.transaction();
        if self.name == "resize" {
            let step = self.phase.min(16 - self.phase);
            transaction.resize(1280 - step * 8, 800 - step * 5, 1.0);
        } else {
            let count = if self.name == "sparse" {
                1
            } else {
                Self::count(self.name)
            };
            for index in 0..count {
                if self.name == "image_replace" {
                    let canvas = image_canvas(replacement_image(index, self.image_revision));
                    transaction
                        .replace_scene(RetainedNodeId::for_owner(index + 2), Rc::new(canvas));
                }
                transaction.set_transform(
                    RetainedNodeId::for_owner(index + 2),
                    Self::transform(self.name, index, self.phase),
                );
            }
        }
        transaction.commit().unwrap();
    }
}

// Encode both index bytes: truncating to u8 silently repeats image contents after 256.
pub(super) fn image(index: u64) -> tileink::Image {
    tileink::Image {
        width: 16,
        height: 16,
        pixels: (0..256u32)
            .map(|pixel| {
                u32::from_le_bytes([
                    ((pixel % 16 * 16) as u8) | ((index >> 8) as u8),
                    (pixel / 16 * 16) as u8,
                    index as u8,
                    255,
                ])
            })
            .collect(),
    }
}

fn image_canvas(image: tileink::Image) -> Canvas {
    let mut canvas = Canvas::new(1280, 800, 1.0);
    canvas
        .push_image(
            Rect::new(0.25, 0.5, 65.75, 23.25),
            Rc::new(image),
            peniko::Extend::Pad,
            tileink::PatternSampling::Bilinear,
        )
        .unwrap();
    canvas
}

pub(super) fn replacement_image(index: u64, revision: u64) -> tileink::Image {
    let mut image = image(index);
    // Opaque RGB encodes all 64 revision bits without changing image dimensions,
    // alpha, or the remaining pixels that distinguish individual resources.
    for (pixel, bytes) in image
        .pixels
        .iter_mut()
        .zip(revision.to_le_bytes().chunks(3))
    {
        let mut rgba = [0, 0, 0, 255];
        rgba[..bytes.len()].copy_from_slice(bytes);
        *pixel = u32::from_le_bytes(rgba);
    }
    image
}

#[cfg(test)]
mod tests {
    #[test]
    fn replacement_revision_survives_geometry_cycle_boundaries() {
        let mut workload = super::Workload::new("image_replace");
        for revision in 1..=33 {
            workload.advance();
            assert_eq!(workload.image_revision, revision);
            assert_eq!(u64::from(workload.phase), revision % 16);
        }
    }
}
