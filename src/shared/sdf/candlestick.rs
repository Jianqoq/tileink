use peniko::kurbo::Point;

use super::rect::{Radius, Rect};
use crate::{TILE_SIZE, shared::bounds::Bounds};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CandleStick {
    pub center_x: f32,
    pub high_y: f32,
    pub low_y: f32,
    pub body_top_y: f32,
    pub body_bottom_y: f32,
    pub body_width: u32,
    pub wick_width: u32,
}

impl CandleStick {
    pub fn new(
        center_x: f32,
        high_y: f32,
        low_y: f32,
        body_top_y: f32,
        body_bottom_y: f32,
        body_width: u32,
        wick_width: u32,
    ) -> Self {
        assert!(
            Self::valid_body_width(body_width),
            "candlestick body width must be positive"
        );
        assert!(
            Self::valid_wick_width(wick_width),
            "candlestick wick width must be positive"
        );
        Self {
            center_x,
            high_y,
            low_y,
            body_top_y,
            body_bottom_y,
            body_width,
            wick_width,
        }
    }

    pub const fn valid_body_width(body_width: u32) -> bool {
        body_width > 0
    }

    pub const fn valid_wick_width(wick_width: u32) -> bool {
        wick_width > 0
    }

    pub(crate) fn bounds(self) -> Bounds {
        let (body_x0, body_y0, body_x1, body_y1) = self.body_axis_bounds();
        let (wick_x0, wick_x1) = self.wick_x_bounds();
        let wick_y0 = self.high_y.min(self.low_y);
        let wick_y1 = self.high_y.max(self.low_y);
        Bounds::new(
            body_x0.min(wick_x0).floor() as i32,
            body_y0.min(wick_y0).floor() as i32,
            body_x1.max(wick_x1).ceil() as i32,
            body_y1.max(wick_y1).ceil() as i32,
        )
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.center_x -= dx;
        self.high_y -= dy;
        self.low_y -= dy;
        self.body_top_y -= dy;
        self.body_bottom_y -= dy;
        self
    }

    pub(crate) fn tile_is_solid(self, _: Bounds) -> bool {
        false
    }

    pub(crate) fn fine_area(
        self,
        area: &mut [f32; (TILE_SIZE * TILE_SIZE) as usize],
        tile_bounds: Bounds,
        pixel_bounds: Bounds,
    ) {
        let mut body = [0.0; (TILE_SIZE * TILE_SIZE) as usize];
        self.wick_rect().fine_area(area, tile_bounds, pixel_bounds);
        self.body_rect()
            .fine_area(&mut body, tile_bounds, pixel_bounds);
        for (dst, src) in area.iter_mut().zip(body) {
            *dst = dst.max(src);
        }
    }

    fn wick_rect(self) -> Rect {
        let (x0, x1) = self.wick_x_bounds();
        Rect {
            start: Point::new(f64::from(x0), f64::from(self.high_y.min(self.low_y))),
            end: Point::new(f64::from(x1), f64::from(self.high_y.max(self.low_y))),
            radius: Radius::ZERO,
        }
    }

    fn body_rect(self) -> Rect {
        let (x0, y0, x1, y1) = self.body_axis_bounds();
        Rect {
            start: Point::new(f64::from(x0), f64::from(y0)),
            end: Point::new(f64::from(x1), f64::from(y1)),
            radius: Radius::ZERO,
        }
    }

    fn body_axis_bounds(self) -> (f32, f32, f32, f32) {
        let half_width = self.body_width as f32 * 0.5;
        let x0 = self.center_x - half_width;
        let x1 = self.center_x + half_width;
        let mut y0 = self.body_top_y.min(self.body_bottom_y);
        let mut y1 = self.body_top_y.max(self.body_bottom_y);
        if y0 == y1 {
            y0 -= 0.5;
            y1 += 0.5;
        }
        (x0, y0, x1, y1)
    }

    fn wick_x_bounds(self) -> (f32, f32) {
        let half_width = self.wick_width as f32 * 0.5;
        (self.center_x - half_width, self.center_x + half_width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_include_wick_and_body() {
        let candle = CandleStick::new(16.5, 4.0, 28.0, 10.0, 22.0, 7, 1);

        assert_eq!(candle.bounds(), Bounds::new(13, 4, 20, 28));
    }

    #[test]
    fn even_body_width_is_valid_and_keeps_one_unit_wick() {
        let candle = CandleStick::new(16.5, 4.0, 28.0, 10.0, 22.0, 8, 1);

        assert!(CandleStick::valid_body_width(8));
        assert!(!CandleStick::valid_body_width(0));
        assert!(CandleStick::valid_wick_width(1));
        assert!(!CandleStick::valid_wick_width(0));
        assert_eq!(candle.bounds(), Bounds::new(12, 4, 21, 28));
        assert_eq!(candle.wick_rect().start.x, 16.0);
        assert_eq!(candle.wick_rect().end.x, 17.0);
    }

    #[test]
    fn wick_width_is_user_controlled() {
        let candle = CandleStick::new(16.5, 4.0, 28.0, 10.0, 22.0, 1, 6);

        assert_eq!(candle.bounds(), Bounds::new(13, 4, 20, 28));
        assert_eq!(candle.wick_rect().start.x, 13.5);
        assert_eq!(candle.wick_rect().end.x, 19.5);
    }

    #[test]
    fn doji_body_expands_to_one_pixel_line() {
        let candle = CandleStick::new(16.5, 8.0, 24.0, 14.0, 14.0, 5, 1);

        assert_eq!(candle.bounds(), Bounds::new(14, 8, 19, 24));
    }
}
