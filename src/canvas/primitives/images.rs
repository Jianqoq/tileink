use super::*;

impl Canvas {
    /// Adds an external image scaled into `rect` with explicit extend and sampling.
    ///
    /// The image is stored as a pattern brush, so it uses the same upload/sampling
    /// path as SVG raster images. Empty images, empty rectangles, and non-finite
    /// rectangles are ignored.
    pub fn push_image(
        &mut self,
        rect: Rect,
        image: impl Into<SharedRc<Image>>,
        extend: Extend,
        sampling: PatternSampling,
    ) -> Option<DrawId> {
        if !image_rect_is_valid(rect) {
            return None;
        }
        let image = image.into();
        if image.width == 0 || image.height == 0 {
            return None;
        }
        let key = self.register_scene_image(image)?;
        let brush = Brush::from_scene_image_key_with_options(key, rect, extend, sampling, 255)?;
        Some(self.push_rect(rect, Radius::ZERO, brush))
    }

    /// Adds a renderer-owned image resource scaled into `rect` with explicit extend and sampling.
    ///
    /// The scene only stores `key`; the renderer resolves the image through
    /// its resource table at render time.
    pub fn push_image_key(
        &mut self,
        rect: Rect,
        key: ImageKey,
        extend: Extend,
        sampling: PatternSampling,
    ) -> Option<DrawId> {
        let brush = Brush::from_image_key_with_options(key, rect, extend, sampling, 255)?;
        Some(self.push_rect(rect, Radius::ZERO, brush))
    }

    pub(crate) fn register_scene_image(
        &mut self,
        image: impl Into<crate::shared::image_resource::ImageSource>,
    ) -> Option<ImageKey> {
        let image = image.into();
        let key = ImageKey::new(image.identity().1);
        if self.scene_images.contains(key) {
            return Some(key);
        }
        self.scene_images.insert(key, image).then_some(key)
    }

    pub(crate) fn scene_image_resources(&self) -> &ImageResourceStore {
        &self.scene_images
    }
}
