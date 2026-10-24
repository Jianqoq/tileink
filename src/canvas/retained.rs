mod damage;
mod damage_buffer;
mod types;

pub use types::RetainedNodeId;
pub(crate) use types::{
    NodeRevision, PersistentLayerKey, RetainedDamage, RetainedFrame, RetainedFrameDelta,
    RetainedNodeKind, RetainedNodePatch, RetainedNodeState, RetainedSurfaceId,
};
