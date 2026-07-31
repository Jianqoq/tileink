use crate::shared::bounds::Bounds;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowOptions {
    pub offset_x: f32,
    pub offset_y: f32,
    /// Exponential falloff distance in pixels. Four expand lengths cover more
    /// than 98% of the visible soft shadow while keeping tile bounds finite.
    pub expand: f32,
    pub intensity: f32,
}

impl ShadowOptions {
    pub fn new(offset_x: f32, offset_y: f32, expand: f32, intensity: f32) -> Self {
        Self {
            offset_x,
            offset_y,
            expand,
            intensity,
        }
    }

    pub(crate) fn normalized(self) -> Option<Self> {
        let intensity = self.intensity.clamp(0.0, 1.0);
        (intensity > 0.0).then_some(Self {
            expand: self.expand.max(0.0),
            intensity,
            ..self
        })
    }
}

pub(crate) fn shadow_bounds(bounds: Bounds, options: ShadowOptions) -> Bounds {
    let outset = options.expand * 4.0 + 1.0;
    Bounds::new(
        (bounds.x0 as f32 + options.offset_x - outset).floor() as i32,
        (bounds.y0 as f32 + options.offset_y - outset).floor() as i32,
        (bounds.x1 as f32 + options.offset_x + outset).ceil() as i32,
        (bounds.y1 as f32 + options.offset_y + outset).ceil() as i32,
    )
}
