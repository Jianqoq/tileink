use std::{cell::OnceCell, ops::Deref};

use crate::{Canvas, shared::bounds::Bounds};

/// Chunk geometry is read by damage collection and frame construction. Cache
/// its unclipped visual bounds until the encoded canvas changes. Mutable access
/// is explicit so every geometry, transform and surface edit invalidates it.
pub(crate) struct ChunkCanvas {
    canvas: Canvas,
    bounds: OnceCell<Bounds>,
    #[cfg(test)]
    evaluations: std::cell::Cell<usize>,
}

impl From<Canvas> for ChunkCanvas {
    fn from(canvas: Canvas) -> Self {
        Self {
            canvas,
            bounds: OnceCell::new(),
            #[cfg(test)]
            evaluations: std::cell::Cell::new(0),
        }
    }
}

// Read access is unrestricted. Deliberately omit DerefMut: edits must clear the
// cached bounds before a caller can mutate any part of the encoded canvas.
impl Deref for ChunkCanvas {
    type Target = Canvas;

    fn deref(&self) -> &Canvas {
        &self.canvas
    }
}

impl ChunkCanvas {
    pub(super) fn edit(&mut self) -> &mut Canvas {
        self.bounds.take();
        &mut self.canvas
    }

    pub(super) fn visual_bounds(&self) -> Bounds {
        *self.bounds.get_or_init(|| self.compute_bounds())
    }

    fn compute_bounds(&self) -> Bounds {
        #[cfg(test)]
        self.evaluations.set(self.evaluations.get() + 1);
        self.canvas.visual_bounds()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Radius;
    use peniko::{Color, kurbo::Rect};

    #[test]
    fn repeated_visual_bounds_queries_traverse_geometry_once() {
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.push_rect(Rect::new(2.0, 3.0, 8.0, 9.0), Radius::ZERO, Color::WHITE);
        let chunk = ChunkCanvas::from(canvas);
        let expected = chunk.canvas.visual_bounds();
        assert_eq!(chunk.visual_bounds(), expected);
        assert_eq!(chunk.visual_bounds(), expected);
        assert_eq!(chunk.evaluations.get(), 1);
    }

    #[test]
    fn mutable_canvas_access_invalidates_empty_and_populated_bounds() {
        let mut chunk = ChunkCanvas::from(Canvas::new(64, 64, 1.0));
        assert!(chunk.visual_bounds().is_empty());
        chunk
            .edit()
            .push_rect(Rect::new(2.0, 3.0, 8.0, 9.0), Radius::ZERO, Color::WHITE);
        assert_eq!(chunk.visual_bounds(), chunk.canvas.visual_bounds());
        assert_eq!(chunk.evaluations.get(), 2);
        let first = chunk.visual_bounds();
        chunk.edit().push_rect(
            Rect::new(24.0, 25.0, 40.0, 41.0),
            Radius::ZERO,
            Color::WHITE,
        );
        let expanded = chunk.visual_bounds();
        assert_ne!(expanded, first);
        assert_eq!(expanded, chunk.canvas.visual_bounds());
        assert_eq!(chunk.visual_bounds(), expanded);
        assert_eq!(chunk.evaluations.get(), 3);
        // Even an edit that leaves geometry unchanged must invalidate; the
        // wrapper cannot depend on callers predicting the effect of mutation.
        let _ = chunk.edit();
        assert_eq!(chunk.visual_bounds(), expanded);
        assert_eq!(chunk.evaluations.get(), 4);
    }
}
