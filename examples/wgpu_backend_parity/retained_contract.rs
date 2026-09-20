//! Shared retained acceptance semantics, independent of GPU backend.
use super::{
    Result,
    retained_sequence::{Frame, Sequence},
};
use tileink::Image;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Owned,
    Transient,
    Persistent,
}
impl Target {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Owned => "owned",
            Self::Transient => "transient",
            Self::Persistent => "persistent",
        }
    }
}

pub(super) fn validate_image(
    name: &str,
    stats: &tileink::IncrementalRenderStats,
    transient: bool,
    full: bool,
    sequence: &Sequence,
    frame: Frame,
    image: &Image,
) -> Result<()> {
    // Independent semantic checks catch shared failures that exact route comparison cannot.
    if image.rgba8_at(0, 0) != sequence.background {
        return Err(format!(
            "{} {}: background sentinel differs: {:?}",
            name,
            frame.name(),
            image.rgba8_at(0, 0)
        )
        .into());
    }
    if matches!(frame, Frame::Empty | Frame::EmptyStatic)
        && image.pixels.iter().any(|pixel| *pixel != 0)
    {
        return Err(format!(
            "{} {}: removed content left stale pixels",
            name,
            frame.name()
        )
        .into());
    }

    if matches!(frame, Frame::Geometry | Frame::Resume) {
        validate_redraw(
            stats,
            if transient {
                Target::Transient
            } else {
                Target::Owned
            },
            full,
        )
        .map_err(|error| format!("{} {}: {error}: {stats:?}", name, frame.name()))?;
    }
    if !full && !transient {
        if matches!(frame, Frame::Static | Frame::EmptyStatic)
            && (stats.dirty_tiles != 0
                || stats.chunks_rebuilt != 0
                || stats.gpu_uploaded_bytes != 0)
        {
            return Err(format!(
                "{} {}: static frame did not reuse history: {stats:?}",
                name,
                frame.name()
            )
            .into());
        }
        if frame == Frame::JournalGap && !stats.full_scene_sync {
            return Err(format!("{}: journal gap did not force full synchronization", name).into());
        }
        if frame == Frame::Resume && stats.full_scene_sync {
            return Err(format!(
                "{}: incremental updates did not resume after journal gap",
                name
            )
            .into());
        }
        if matches!(
            frame,
            Frame::Grow | Frame::Shrink | Frame::Tile15 | Frame::Tile16 | Frame::Tile17
        ) && stats.chunks_rebuilt != 0
        {
            return Err(format!("{}: resize rebuilt unchanged geometry", name).into());
        }
    }
    Ok(())
}

fn validate_redraw(
    stats: &tileink::IncrementalRenderStats,
    target: Target,
    full: bool,
) -> Result<()> {
    if full {
        if !stats.full_redraw || stats.dirty_tiles != stats.total_tiles {
            return Err("ForceFull oracle did not repaint the whole target".into());
        }
    } else if target != Target::Transient
        && (stats.full_redraw || stats.dirty_tiles == 0 || stats.dirty_tiles >= stats.total_tiles)
    {
        return Err("designated Auto frame did not perform a partial redraw".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_and_force_full_execution_cannot_silently_substitute_each_other() {
        let mut stats = tileink::IncrementalRenderStats {
            dirty_tiles: 4,
            total_tiles: 64,
            ..Default::default()
        };
        assert!(validate_redraw(&stats, Target::Owned, false).is_ok());
        assert!(validate_redraw(&stats, Target::Persistent, true).is_err());
        stats.full_redraw = true;
        stats.dirty_tiles = 64;
        assert!(validate_redraw(&stats, Target::Persistent, false).is_err());
        assert!(validate_redraw(&stats, Target::Owned, true).is_ok());
        stats.full_redraw = false;
        stats.dirty_tiles = 0;
        assert!(validate_redraw(&stats, Target::Owned, false).is_err());
    }
}
