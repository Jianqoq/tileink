//! Compiled composites contain only operations that own a surface.
//! Clip state lives in batch stacks; only geometry groups own hidden draws.
use crate::shared::layer::{filter::Filter, region::Region};
use peniko::BlendMode;

#[derive(Clone, Debug)]
pub(crate) enum OffscreenLayer {
    Isolate {
        draw: usize,
    },
    Opacity {
        draw: usize,
        opacity: f32,
    },
    Blend {
        draw: usize,
        mode: BlendMode,
    },
    Filter {
        filter: Filter,
        sample_region: Region,
    },
    Backdrop {
        filter: Filter,
        sample_region: Region,
    },
}
