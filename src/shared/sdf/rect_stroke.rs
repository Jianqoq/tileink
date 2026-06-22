use crate::shared::sdf::rect::Rect;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RectStroke {
    pub outer: Rect,
    pub inner: Rect,
}