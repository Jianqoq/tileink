use super::*;
use ::windows::{
    Win32::{Foundation::E_NOTIMPL, Graphics::DirectWrite::*},
    core::{Error, Result},
};
use rustc_hash::FxHashMap;
mod font;
mod mask;

#[derive(Default)]
pub(super) struct Backend {
    context: Option<Result<Box<Context>>>,
}
impl Backend {
    pub(super) fn image(
        &mut self,
        fonts: &FontSystem,
        key: CacheKey,
        mode: TextSubpixelMode,
    ) -> Result<Option<GlyphRasterImage>> {
        match self
            .context
            .get_or_insert_with(|| Context::new().map(Box::new))
        {
            Ok(context) => context.image(fonts, key, mode),
            Err(error) => Err(error.clone()),
        }
    }
    pub(super) fn clear(&mut self) {
        if let Some(Ok(context)) = &mut self.context {
            context.fonts.clear();
        }
    }
}

struct Context {
    factory: IDWriteFactory2,
    loader: IDWriteInMemoryFontFileLoader,
    fonts: FxHashMap<cosmic_text::fontdb::ID, Option<font::Resource>>,
}
impl Context {
    fn new() -> Result<Self> {
        // SAFETY: DirectWrite's shared factory is independent of HWND/GPU ownership.
        // Registration lives until Context drops, after all font resources are released.
        unsafe {
            let factory: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let loader = factory.CreateInMemoryFontFileLoader()?;
            let base: IDWriteFactory2 = ::windows::core::Interface::cast(&factory)?;
            base.RegisterFontFileLoader(&loader)?;
            Ok(Self {
                factory: base,
                loader,
                fonts: FxHashMap::default(),
            })
        }
    }
    fn image(
        &mut self,
        fonts: &FontSystem,
        key: CacheKey,
        mode: TextSubpixelMode,
    ) -> Result<Option<GlyphRasterImage>> {
        let factory = &self.factory;
        let loader = &self.loader;
        let resource = self
            .fonts
            .entry(key.font_id)
            .or_insert_with(|| {
                fonts
                    .db()
                    .with_face_data(key.font_id, |bytes, index| {
                        font::Resource::new(factory, loader, bytes, index)
                    })
                    .and_then(Result::ok)
            })
            .as_mut()
            .ok_or_else(|| Error::from_hresult(E_NOTIMPL))?;
        let face = resource.face(key.font_weight)?;
        mask::image(&self.factory, &face, key, mode)
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        self.fonts.clear();
        // SAFETY: Registration succeeded in new; masks own bytes, not COM references.
        // Teardown has no error channel; releasing this factory also releases registration.
        let _ = unsafe { self.factory.UnregisterFontFileLoader(&self.loader) };
    }
}
