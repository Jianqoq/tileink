use peniko::kurbo::{Affine, BezPath, Point, Rect, Shape};
use peniko::Color;

use crate::shared::sdf::rect::Radius;

/// Geometry that defines where a backdrop filter is visible.
///
/// This region is independent of content drawn inside the backdrop layer.
#[derive(Clone, Debug)]
pub enum BackdropRegion {
    Rect {
        rect: Rect,
        radius: Radius,
    },
    Path {
        path: BezPath,
        transform: Affine,
        tolerance: f64,
    },
}

impl BackdropRegion {
    pub fn rect(rect: Rect, radius: Radius) -> Self {
        Self::Rect { rect, radius }
    }

    pub fn path(path: BezPath, transform: Affine, tolerance: f64) -> Self {
        Self::Path {
            path,
            transform,
            tolerance,
        }
    }

    // pub(crate) fn mask_command(&self) -> Command {
    //     match self {
    //         Self::Rect { rect, radius } if radius.is_zero() => Command::FillRect(FillRect {
    //             rect: *rect,
    //             color: Color::WHITE,
    //         }),
    //         Self::Rect { rect, radius } => Command::Sdf {
    //             sdf: Sdf::Rect(SdfRect {
    //                 start: Point::new(rect.x0, rect.y0),
    //                 end: Point::new(rect.x1, rect.y1),
    //                 radius: *radius,
    //             }),
    //             brush: crate::paint::Brush::Solid(Color::WHITE),
    //         },
    //         Self::Path {
    //             path,
    //             transform,
    //             tolerance,
    //         } => {
    //             let bounds = path.bounding_box();
    //             Command::Fill(Fill {
    //                 path: path.clone(),
    //                 brush: Brush::Solid(Color::WHITE),
    //                 transform: *transform,
    //                 rule: FillRule::NonZero,
    //                 tolerance: *tolerance,
    //                 bounds: Bounds {
    //                     x0: bounds.x0 as i32,
    //                     y0: bounds.y0 as i32,
    //                     x1: bounds.x1 as i32,
    //                     y1: bounds.y1 as i32,
    //                 },
    //             })
    //         }
    //     }
    // }
}
