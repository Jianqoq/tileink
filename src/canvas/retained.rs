mod damage;
mod types;

pub use types::RetainedNodeId;
pub(crate) use types::{
    NodeGeneration, PersistentLayerKey, RetainedDamage, RetainedFrame, RetainedFrameDelta,
    RetainedNodeKind, RetainedNodePatch, RetainedNodeState, RetainedSurfaceId,
};
