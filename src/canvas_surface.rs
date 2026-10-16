//! A coherent canvas viewport, preserving either authored logical size or native backing size.

/// Validated viewport shared by immediate canvases and retained scenes.
///
/// Native windows must use [`Self::from_physical`]: rounding to logical integers before
/// reconstructing backing pixels changes the raster extent at fractional DPI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasSurface {
    logical: (f64, f64),
    physical: (u32, u32),
    scale: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidCanvasSurface;

impl std::fmt::Display for InvalidCanvasSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("canvas extent or pixels-per-point scale is invalid or unrepresentable")
    }
}

impl std::error::Error for InvalidCanvasSurface {}

impl CanvasSurface {
    /// Creates an authored logical viewport; its backing allocation rounds outward.
    ///
    /// # Errors
    /// Returns an error for an invalid scale or an overflowing backing extent.
    pub fn from_logical(logical: (u32, u32), scale: f32) -> Result<Self, InvalidCanvasSurface> {
        Self::validate_scale(scale)?;
        let extent = |value| {
            let physical = (f64::from(value) * f64::from(scale)).ceil();
            if physical > f64::from(u32::MAX) {
                return Err(InvalidCanvasSurface);
            }
            Ok(physical as u32)
        };
        Ok(Self {
            logical: (f64::from(logical.0), f64::from(logical.1)),
            physical: (extent(logical.0)?, extent(logical.1)?),
            scale,
        })
    }

    /// Creates a native viewport without quantizing its logical size or recomputing its pixels.
    ///
    /// # Errors
    /// Returns an error for an invalid scale or a logical extent outside f32 drawing coordinates.
    pub fn from_physical(physical: (u32, u32), scale: f32) -> Result<Self, InvalidCanvasSurface> {
        Self::validate_scale(scale)?;
        let logical = (
            f64::from(physical.0) / f64::from(scale),
            f64::from(physical.1) / f64::from(scale),
        );
        if logical.0 > f64::from(f32::MAX) || logical.1 > f64::from(f32::MAX) {
            return Err(InvalidCanvasSurface);
        }
        Ok(Self {
            logical,
            physical,
            scale,
        })
    }

    fn validate_scale(scale: f32) -> Result<(), InvalidCanvasSurface> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(InvalidCanvasSurface);
        }
        Ok(())
    }

    #[must_use]
    pub const fn logical_size(self) -> (f64, f64) {
        self.logical
    }
    #[must_use]
    pub const fn physical_size(self) -> (u32, u32) {
        self.physical
    }
    #[must_use]
    pub const fn scale_factor(self) -> f32 {
        self.scale
    }
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.physical.0 == 0 || self.physical.1 == 0
    }
}
