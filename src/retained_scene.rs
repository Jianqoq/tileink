//! Retained scene state and its incremental materialization pipeline.
//!
//! The implementation is split by responsibility so the public scene API stays small while
//! materializer, mutation, and validation code can evolve independently.

mod prelude {
    pub(crate) use std::{
        collections::{BTreeMap, VecDeque},
        error::Error,
        fmt,
        rc::Rc,
        sync::atomic::{AtomicU64, Ordering},
    };

    pub(crate) use peniko::{
        Compose, Mix,
        kurbo::{Affine, BezPath, PathEl, Point, Rect},
    };
    pub(crate) use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

    pub(crate) use crate::{
        Canvas, FillRule, Filter, Mask, NodeGeneration, PersistentLayerKey, Region, RetainedNodeId,
        Sdf,
        canvas::{
            PainterKey, RetainedFrameDelta, RetainedNodeKind, RetainedNodePatch, SceneBufferChanges,
        },
        shared::{
            bounds::Bounds,
            draw_record::{DrawRecord, DrawTagWord, FillRuleWord},
            execution::{Command, CommandList, RetainedBatchBranch},
            image::Image,
            image_resource::{ImageKey, ImageResourceStore},
            layer::{Layer, filter},
            line::Line,
            path::PathRecord,
            scene_arena::{ArenaAllocation, SceneArena},
        },
        text::{CanvasGlyph, TextRun},
    };
}

#[cfg(test)]
pub(crate) use prelude::*;

mod materializer;
mod model;
mod scene;
mod transaction;
mod validation;

pub(crate) use materializer::PersistentSceneMaterializer;
#[cfg(test)]
pub(crate) use model::NodeKind;
#[cfg(test)]
pub(crate) use scene::JOURNAL_CAPACITY;

pub use model::{
    RetainedChildBranch, RetainedLayerDescriptor, RetainedParent, RetainedSceneError, SceneVersion,
};
pub use scene::RetainedScene;
pub use transaction::RetainedSceneTransaction;

#[cfg(feature = "bench-internals")]
pub use materializer::RetainedMaterializerBenchmark;

#[cfg(test)]
#[path = "retained_scene/tests.rs"]
mod tests;
