use crate::TILE_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Bounds {
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn canvas(width: u32, height: u32) -> Self {
        Self::new(0, 0, width as i32, height as i32)
    }
    pub fn intersect(&self, b: Bounds) -> Bounds {
        Bounds {
            x0: self.x0.max(b.x0),
            y0: self.y0.max(b.y0),
            x1: self.x1.min(b.x1),
            y1: self.y1.min(b.y1),
        }
    }

    pub fn outset(self, amount: i32) -> Bounds {
        Bounds {
            x0: self.x0 - amount,
            y0: self.y0 - amount,
            x1: self.x1 + amount,
            y1: self.y1 + amount,
        }
    }

    pub fn union(self, other: Bounds) -> Bounds {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Bounds {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    pub fn is_empty(self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }

    pub fn width(self) -> u32 {
        (self.x1 - self.x0).max(0) as u32
    }

    pub fn height(self) -> u32 {
        (self.y1 - self.y0).max(0) as u32
    }
}

/// Pixel-space bounds of a transformed path.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PixelBounds {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl PixelBounds {
    pub fn union(self, other: PixelBounds) -> PixelBounds {
        PixelBounds {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    pub fn tile_bbox(&self, width_in_tiles: u32, height_in_tiles: u32) -> TileBbox {
        let tile_x0 = (self.x0.max(0) as u32 / TILE_SIZE).min(width_in_tiles);
        let tile_y0 = (self.y0.max(0) as u32 / TILE_SIZE).min(height_in_tiles);
        let tile_x1 = (self.x1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(width_in_tiles);
        let tile_y1 = (self.y1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(height_in_tiles);
        TileBbox {
            x0: tile_x0,
            y0: tile_y0,
            x1: tile_x1,
            y1: tile_y1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileBbox {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl TileBbox {
    pub fn tile_stride(&self) -> u32 {
        self.x1.saturating_sub(self.x0)
    }

    pub fn tile_height(&self) -> u32 {
        self.y1.saturating_sub(self.y0)
    }

    pub fn tile_count(&self) -> u32 {
        self.tile_stride() * self.tile_height()
    }
}
