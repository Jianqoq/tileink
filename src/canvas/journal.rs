//! Dirty ranges are valid only for a consumer that observed their source version.
use super::Canvas;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MaterializationStamp {
    materialization: u64,
    version: u64,
}

impl Canvas {
    pub(crate) fn materialization_stamp(&self) -> Option<MaterializationStamp> {
        Some(MaterializationStamp {
            materialization: self.materialization_key?,
            version: self.persistent_frame.as_ref()?.version,
        })
    }

    /// CPU preparation and accepted GPU inputs advance independently. Each cache
    /// must check its own stamp before consuming the latest materializer journal.
    pub(crate) fn journal_follows(&self, previous: Option<MaterializationStamp>) -> bool {
        let (Some(previous), Some(current)) = (previous, self.materialization_stamp()) else {
            return false;
        };
        previous == current
            || (previous.materialization == current.materialization
                && self.buffer_changes.as_ref().is_some_and(|changes| {
                    !changes.full_scene_sync && changes.from_version == Some(previous.version)
                }))
    }
}
