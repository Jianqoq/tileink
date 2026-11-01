use crate::text::{raster::*, scaler::FAKE_ITALIC_ANGLE, *};
use ::windows::{
    Win32::{
        Foundation::{E_INVALIDARG, E_OUTOFMEMORY},
        Graphics::DirectWrite::*,
    },
    core::{BOOL, Error, Result},
};
use cosmic_text::{CacheKey, CacheKeyFlags};
use std::mem::ManuallyDrop;

pub(super) fn image(
    factory: &IDWriteFactory2,
    face: &IDWriteFontFace,
    key: CacheKey,
    mode: TextSubpixelMode,
) -> Result<Option<GlyphRasterImage>> {
    let size = f32::from_bits(key.font_size_bits);
    if !size.is_finite() || size <= 0.0 {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    // DirectWrite substitutes .notdef for an invalid glyph ID. Our cache contract
    // represents a missing glyph as no image, matching the portable rasterizer.
    // SAFETY: The live face exposes its immutable, validated glyph count.
    if key.glyph_id >= unsafe { face.GetGlyphCount() } {
        return Ok(None);
    }
    let offset = raster_glyph_offset(key);
    let transform = key
        .flags
        .contains(CacheKeyFlags::FAKE_ITALIC)
        .then(|| DWRITE_MATRIX {
            m11: 1.0,
            m12: 0.0,
            m21: -FAKE_ITALIC_ANGLE.to_radians().tan(),
            m22: 1.0,
            dx: 0.0,
            dy: 0.0,
        });
    let mut run = DWRITE_GLYPH_RUN {
        fontFace: ManuallyDrop::new(Some(face.clone())),
        fontEmSize: size,
        glyphCount: 1,
        glyphIndices: &raw const key.glyph_id,
        glyphAdvances: std::ptr::null(),
        glyphOffsets: std::ptr::null(),
        isSideways: BOOL(0),
        bidiLevel: 0,
    };
    let gray = mode == TextSubpixelMode::None;
    // SAFETY: The run, glyph and optional transform remain valid throughout this call;
    // the returned analysis retains the native resources it needs.
    let result = unsafe {
        factory.CreateGlyphRunAnalysis(
            &raw const run,
            transform.as_ref().map(std::ptr::from_ref),
            DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
            DWRITE_MEASURING_MODE_NATURAL,
            if key.flags.contains(CacheKeyFlags::DISABLE_HINTING) {
                DWRITE_GRID_FIT_MODE_DISABLED
            } else {
                DWRITE_GRID_FIT_MODE_ENABLED
            },
            if gray {
                DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE
            } else {
                DWRITE_TEXT_ANTIALIAS_MODE_CLEARTYPE
            },
            offset.x,
            offset.y,
        )
    };
    // SAFETY: Drops exactly the owned clone placed in this ABI's ManuallyDrop field.
    unsafe { ManuallyDrop::drop(&mut run.fontFace) };
    let analysis = result?;
    let kind = if gray {
        DWRITE_TEXTURE_ALIASED_1x1
    } else {
        DWRITE_TEXTURE_CLEARTYPE_3x1
    };
    // SAFETY: The analysis is live and the texture type matches its antialias mode.
    let bounds = unsafe { analysis.GetAlphaTextureBounds(kind)? };
    let width = u32::try_from(i64::from(bounds.right) - i64::from(bounds.left))
        .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
    let height = u32::try_from(i64::from(bounds.bottom) - i64::from(bounds.top))
        .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
    if width == 0 || height == 0 {
        return Ok(None);
    }
    let length = texture_length(width, height, gray)?;
    let top = bounds
        .top
        .checked_neg()
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
    let mut data = Vec::new();
    data.try_reserve_exact(length)
        .map_err(|_| Error::from_hresult(E_OUTOFMEMORY))?;
    data.resize(length, 0);
    // SAFETY: The writable slice is sized from these exact bounds, with checked arithmetic.
    unsafe { analysis.CreateAlphaTexture(kind, &raw const bounds, &mut data)? };
    if mode == TextSubpixelMode::Bgr {
        for pixel in data.as_chunks_mut::<3>().0 {
            pixel.swap(0, 2);
        }
    }
    Ok(Some(GlyphRasterImage::new(
        if gray {
            GlyphContent::Mask
        } else {
            GlyphContent::SubpixelMask
        },
        swash::zeno::Placement {
            left: bounds.left,
            top,
            width,
            height,
        },
        data,
    )))
}

fn texture_length(width: u32, height: u32, gray: bool) -> Result<usize> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(if gray { 1 } else { 3 }))
        .filter(|&length| u32::try_from(length).is_ok())
        .ok_or_else(|| Error::from_hresult(E_OUTOFMEMORY))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_texture_lengths_fit_the_native_u32_buffer_contract() {
        assert_eq!(texture_length(7, 9, true).unwrap(), 63);
        assert_eq!(texture_length(7, 9, false).unwrap(), 189);
        // The ABI accepts a u32 byte length even on a 64-bit host. Reject before
        // allocation rather than letting the generated FFI wrapper panic.
        assert!(texture_length(u32::MAX, 2, true).is_err());
        assert!(texture_length(u32::MAX, 1, false).is_err());
    }
}
