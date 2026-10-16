use super::*;

/// Retained painter order and batch membership are initialized and updated together.
/// Immediate canvases own empty vectors and take their order from the execution plan.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PainterMetadata {
    pub(crate) keys: Vec<PainterKey>,
    pub(crate) batch_ids: Vec<u32>,
    /// Live physical draw count per stable batch, including dead historical placeholders.
    pub(crate) batch_counts: Vec<u32>,
}

impl Canvas {
    pub(crate) fn retained_painter(&self) -> Option<&PainterMetadata> {
        self.persistent_root.map(|_| &self.painter)
    }

    /// Retained edits own live membership in painter metadata while reusing plan structure.
    /// Immediate canvases take membership from their current compilation.
    pub(crate) fn raster_batch_ids<'a>(&'a self, plan: &'a ExecPlan) -> &'a [u32] {
        match self.retained_painter() {
            Some(painter) => &painter.batch_ids,
            None => &plan.draw_batch_ids,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct PainterKey {
    pub(crate) path: SharedRc<[u128]>,
    pub(crate) local: u32,
}

impl PainterKey {
    pub(crate) fn inactive() -> Self {
        Self {
            path: SharedRc::from([u128::MAX]),
            local: u32::MAX,
        }
    }
}
