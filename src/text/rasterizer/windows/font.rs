use ::windows::{
    Win32::{Foundation::E_NOTIMPL, Graphics::DirectWrite::*},
    core::{BOOL, Error, Interface, Result},
};
use cosmic_text::Weight;
use rustc_hash::FxHashMap;

/// One copied font blob per database face, with weight instances reusing its definition.
pub(super) struct Resource {
    base: IDWriteFontFace,
    definition: IDWriteFontResource,
    axes: Vec<DWRITE_FONT_AXIS_VALUE>,
    ranges: Vec<DWRITE_FONT_AXIS_RANGE>,
    faces: FxHashMap<u32, IDWriteFontFace>,
}
impl Resource {
    pub(super) fn new(
        factory: &IDWriteFactory2,
        loader: &IDWriteInMemoryFontFileLoader,
        bytes: &[u8],
        index: u32,
    ) -> Result<Self> {
        let length = u32::try_from(bytes.len()).map_err(|_| Error::from_hresult(E_NOTIMPL))?;
        // SAFETY: The input slice remains valid for this call. A null owner asks the
        // loader to copy the data; no font-database borrow escapes into native objects.
        let file = unsafe {
            loader.CreateInMemoryFontFileReference(
                factory,
                bytes.as_ptr().cast(),
                length,
                None::<&::windows::core::IUnknown>,
            )?
        };
        let mut supported = BOOL(0);
        let mut file_type = DWRITE_FONT_FILE_TYPE_UNKNOWN;
        let mut face_type = DWRITE_FONT_FACE_TYPE_UNKNOWN;
        let mut count = 0;
        // SAFETY: All analysis outputs are valid, writable locals.
        unsafe {
            file.Analyze(
                &raw mut supported,
                &raw mut file_type,
                Some(&raw mut face_type),
                &raw mut count,
            )?;
        };
        if !supported.as_bool() || index >= count {
            return Err(Error::from_hresult(E_NOTIMPL));
        }
        // SAFETY: Analysis validated the type and collection index; the returned face
        // retains its font-file reference while the loader stays registered.
        let base = unsafe {
            factory.CreateFontFace(
                face_type,
                &[Some(file)],
                index,
                DWRITE_FONT_SIMULATIONS_NONE,
            )?
        };
        let variable: IDWriteFontFace5 = base.cast()?;
        // SAFETY: These owned interfaces describe the same validated font definition.
        let definition = unsafe { variable.GetFontResource()? };
        // SAFETY: The definition is live; its axis count defines both writable slices.
        let count = unsafe { definition.GetFontAxisCount() } as usize;
        let mut axes = vec![DWRITE_FONT_AXIS_VALUE::default(); count];
        let mut ranges = vec![DWRITE_FONT_AXIS_RANGE::default(); count];
        if count != 0 {
            // SAFETY: Both slices have the axis count returned by this definition.
            unsafe {
                definition.GetDefaultFontAxisValues(&mut axes)?;
                definition.GetFontAxisRanges(&mut ranges)?;
            }
        }
        Ok(Self {
            base,
            definition,
            axes,
            ranges,
            faces: FxHashMap::default(),
        })
    }
    pub(super) fn face(&mut self, weight: Weight) -> Result<IDWriteFontFace> {
        let Some(index) = self
            .axes
            .iter()
            .position(|axis| axis.axisTag == DWRITE_FONT_AXIS_TAG_WEIGHT)
        else {
            return Ok(self.base.clone());
        };
        let range = self
            .ranges
            .iter()
            .find(|range| range.axisTag == DWRITE_FONT_AXIS_TAG_WEIGHT)
            .ok_or_else(|| Error::from_hresult(E_NOTIMPL))?;
        let value = f32::from(weight.0).clamp(range.minValue, range.maxValue);
        // Native instances are identified by their resolved axes, not an unbounded
        // public request. This fixes duplicate resources for equivalent clamped weights.
        let key = value.to_bits();
        if let Some(face) = self.faces.get(&key) {
            return Ok(face.clone());
        }
        let mut axes = self.axes.clone();
        axes[index].value = value;
        // SAFETY: All axis values preserve their font defaults, except a weight
        // clamped to this definition's declared range. No synthetic bold is applied.
        let face: IDWriteFontFace = unsafe {
            self.definition
                .CreateFontFace(DWRITE_FONT_SIMULATIONS_NONE, &axes)?
        }
        .cast()?;
        self.faces.insert(key, face.clone());
        Ok(face)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamped_weights_share_one_native_instance() {
        let context = super::super::Context::new().unwrap();
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/svg/fonts/RobotoFlex.subset.ttf"
        ));
        let mut font = Resource::new(&context.factory, &context.loader, bytes, 0).unwrap();
        let first = font.face(Weight(60_000)).unwrap();
        let second = font.face(Weight(u16::MAX)).unwrap();
        // Requested weights outside the axis range represent the same native face.
        // Cache by the resolved value so public unbounded weights cannot duplicate it.
        assert_eq!(font.faces.len(), 1);
        assert_eq!(
            ::windows::core::Interface::as_raw(&first),
            ::windows::core::Interface::as_raw(&second)
        );
    }
}
