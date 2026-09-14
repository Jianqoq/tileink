//! Scene images carry either real raster bytes or an immutable vector subscene.
//! Deferred vectors remove SVG lowering's GPU dependency; the selected executor
//! must render them with transparent clear before their consuming draw.
use super::Image;
use crate::Canvas;
use std::rc::{Rc, Weak};

#[derive(Clone)]
pub(crate) enum ImageSource {
    Raster(Rc<Image>),
    Vector(Rc<Canvas>),
}

impl ImageSource {
    pub(crate) fn size(&self) -> (u32, u32) {
        match self {
            Self::Raster(image) => (image.width, image.height),
            Self::Vector(canvas) => canvas.physical_size(),
        }
    }

    pub(crate) fn width(&self) -> u32 {
        self.size().0
    }
    pub(crate) fn height(&self) -> u32 {
        self.size().1
    }

    pub(crate) fn raster(&self) -> Option<&Image> {
        match self {
            Self::Raster(image) => Some(image),
            Self::Vector(_) => None,
        }
    }

    pub(crate) fn identity(&self) -> (u64, u64) {
        match self {
            Self::Raster(image) => (0, Rc::as_ptr(image) as usize as u64),
            Self::Vector(canvas) => (1, Rc::as_ptr(canvas) as usize as u64),
        }
    }

    pub(super) fn guard(&self) -> ImageIdentityGuard {
        match self {
            Self::Raster(image) => ImageIdentityGuard::Raster {
                _owner: Rc::downgrade(image),
            },
            Self::Vector(canvas) => ImageIdentityGuard::Vector {
                _owner: Rc::downgrade(canvas),
            },
        }
    }
}

impl From<Image> for ImageSource {
    fn from(image: Image) -> Self {
        Self::Raster(Rc::new(image))
    }
}

impl From<Rc<Image>> for ImageSource {
    fn from(image: Rc<Image>) -> Self {
        Self::Raster(image)
    }
}

impl From<Canvas> for ImageSource {
    fn from(canvas: Canvas) -> Self {
        Self::Vector(Rc::new(canvas))
    }
}

impl From<Rc<Canvas>> for ImageSource {
    fn from(canvas: Rc<Canvas>) -> Self {
        Self::Vector(canvas)
    }
}

/// Pin allocation identity without retaining source pixels or the scene graph.
/// Weak references also prevent a caller's Rc::make_mut from editing a cached identity.
#[derive(Clone)]
pub(super) enum ImageIdentityGuard {
    Raster { _owner: Weak<Image> },
    Vector { _owner: Weak<Canvas> },
}
