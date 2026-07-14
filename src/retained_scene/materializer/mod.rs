pub(crate) use super::model::*;
pub(crate) use super::prelude::*;
pub(crate) use super::scene::RetainedScene;

mod damage;
mod draw_order;
mod helpers;
mod lifecycle;
mod plan;
mod spatial_tiles;
mod storage;

use self::draw_order::LocalDrawOrder;
use self::helpers::inactive_draw;

#[cfg(feature = "bench-internals")]
pub use self::helpers::RetainedMaterializerBenchmark;

#[derive(Clone, Copy, Eq, PartialEq)]
enum NodeKindTag {
    Group,
    Scene,
    Layer,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct MaterializedNodeMetadata {
    instance: u64,
    generation: u64,
    parent: Option<RetainedParent>,
    order_key: Option<u128>,
    kind: NodeKindTag,
}

pub(crate) struct SceneChunk {
    pub(crate) instance: u64,
    pub(crate) generation: u64,
    pub(crate) source_canvas: Option<Rc<Canvas>>,
    pub(crate) transform_bits: Option<[u64; 6]>,
    // Chunks have a single owner. Keeping their mutable encoding behind an Rc made every
    // revision pay an atomic uniqueness check and made newly inserted chunks allocate twice.
    pub(crate) canvas: Canvas,
    lines: ArenaAllocation,
    paths: ArenaAllocation,
    pub(crate) draws: ArenaAllocation,
    brushes: ArenaAllocation,
    pub(crate) sdfs: ArenaAllocation,
    shadows: ArenaAllocation,
    glyphs: Option<ArenaAllocation>,
    runs: Option<ArenaAllocation>,
    backdrops: ArenaAllocation,
    segments: ArenaAllocation,
    plan_fingerprint: u64,
    plain_fragment: bool,
    local_draw_order: LocalDrawOrder,
    // Persistent damage propagation cannot rediscover ordinary backdrop commands by walking the
    // materialized command tree every frame. Cache their translated dependency geometry with the
    // chunk so an earlier node mutation only visits actual backdrop owners.
    pub(crate) backdrop_dependencies: Vec<BackdropDependency>,
}

#[derive(Clone, Copy)]
pub(crate) struct BackdropDependency {
    dependency: Bounds,
    pub(crate) output: Bounds,
    output_outset: i32,
}

#[derive(Clone, Copy)]
pub(crate) struct BoundsInfluence {
    outset: i32,
    clip: Bounds,
}

impl BoundsInfluence {
    fn apply(self, bounds: Bounds) -> Bounds {
        if bounds.is_empty() {
            bounds
        } else {
            bounds.outset(self.outset).intersect(self.clip)
        }
    }

    /// Composes a nearer layer effect before this already accumulated ancestor effect.
    fn with_nearer(self, nearer: Self) -> Self {
        Self {
            outset: nearer.outset.saturating_add(self.outset),
            clip: nearer.clip.outset(self.outset).intersect(self.clip),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct NodeRebuild {
    plan_dirty: bool,
    transform_only: bool,
}

#[derive(Clone, Copy)]
struct SceneChunkLengths {
    lines: usize,
    paths: usize,
    draws: usize,
    brushes: usize,
    sdfs: usize,
    shadows: usize,
    runs: usize,
    backdrops: usize,
    segments: usize,
}

impl SceneChunkLengths {
    fn from_canvas(canvas: &Canvas) -> Self {
        Self {
            lines: canvas.lines.len(),
            paths: canvas.path_records.len(),
            draws: canvas.draw_records.len(),
            brushes: canvas.brush_blob.len(),
            sdfs: canvas.sdf_blob.len(),
            shadows: canvas.sdf_shadow_blob.len(),
            runs: canvas.text_runs.len(),
            backdrops: canvas.backdrop_pool_capacity as usize,
            segments: canvas.tile_cnt as usize,
        }
    }
}

#[derive(Clone)]
struct SceneCommandLocation {
    parent_list: usize,
    command_index: usize,
    fragment_start: usize,
    fragment_count: usize,
}

#[derive(Clone, Copy)]
struct LayerCommandLocation {
    parent_list: usize,
    command_index: usize,
}

struct RootPlanFragment {
    ops: std::ops::Range<usize>,
    command_lists: std::ops::Range<usize>,
    nodes: HashSet<RetainedNodeId>,
    reassigned_batches: HashMap<RetainedNodeId, u32>,
    removed_batch: Option<(usize, crate::shared::execution::ExecOp)>,
}

pub(crate) struct MaterializedArenas {
    pub(crate) lines: SceneArena<Line>,
    pub(crate) paths: SceneArena<PathRecord>,
    pub(crate) draws: SceneArena<DrawRecord>,
    pub(crate) brushes: SceneArena<u32>,
    pub(crate) sdfs: SceneArena<u32>,
    pub(crate) shadows: SceneArena<u32>,
    pub(crate) glyphs: Option<SceneArena<CanvasGlyph>>,
    pub(crate) runs: SceneArena<TextRun>,
    pub(crate) backdrops: SceneArena<u8>,
    pub(crate) segments: SceneArena<u8>,
}

impl Default for MaterializedArenas {
    fn default() -> Self {
        Self {
            lines: SceneArena::new(Line::default()),
            paths: SceneArena::new(PathRecord::default()),
            draws: SceneArena::new(inactive_draw()),
            brushes: SceneArena::new(0),
            sdfs: SceneArena::new(0),
            shadows: SceneArena::new(0),
            glyphs: None,
            runs: SceneArena::new(TextRun {
                glyph_start: 0,
                glyph_count: 0,
            }),
            backdrops: SceneArena::new(0),
            segments: SceneArena::new(0),
        }
    }
}

/// Persistent CPU materialization for the stateful API.
///
/// Scene data lives in stable arenas and only changed nodes are translated or copied. Command
/// metadata is rebuilt when physical allocations or topology change; the later persistent-plan
/// layer consumes the same stable draw slots without changing this storage contract.
pub(crate) struct PersistentSceneMaterializer {
    pub(crate) scene_id: u64,
    pub(crate) version: SceneVersion,
    pub(crate) canvas: Rc<Canvas>,
    pub(crate) chunks: HashMap<RetainedNodeId, SceneChunk>,
    pub(crate) arenas: MaterializedArenas,
    /// Range vectors are owned by the published canvas for one frame, then reclaimed before the
    /// next mutation so arena synchronization retains peak capacity without copying ranges.
    buffer_changes_scratch: SceneBufferChanges,
    plan_cache_key: u64,
    scene_command_locations: HashMap<RetainedNodeId, SceneCommandLocation>,
    layer_command_locations: HashMap<RetainedNodeId, LayerCommandLocation>,
    resource_refs: HashMap<ImageKey, (Rc<Image>, usize)>,
    pub(crate) dependency_free: bool,
    layer_nodes: HashSet<RetainedNodeId>,
    pub(crate) nonlocal_dependencies: HashSet<RetainedNodeId>,
    pub(crate) surface_dependent_plans: HashSet<RetainedNodeId>,
    painter_bases: HashMap<RetainedNodeId, Rc<[u128]>>,
    painter_parents: HashMap<RetainedNodeId, RetainedParent>,
    flat_plan_has_draws: bool,
    node_bounds: HashMap<RetainedNodeId, Bounds>,
    pub(crate) raw_node_bounds: HashMap<RetainedNodeId, Bounds>,
    /// Conservative fixed domains for bounded translations. Keeping these out of every tile's
    /// hash set avoids duplicating a chart-sized bound for each moving retained chunk.
    pub(crate) bounded_node_bounds: HashMap<RetainedNodeId, Bounds>,
    pub(crate) bounded_raw_node_bounds: HashMap<RetainedNodeId, Bounds>,
    pub(crate) node_tiles: Vec<HashSet<RetainedNodeId>>,
    spatial_nodes: HashSet<RetainedNodeId>,
    raw_node_tiles: Vec<HashSet<RetainedNodeId>>,
    pub(crate) spatial_tiles_size: (u32, u32),
    pub(crate) surface_metadata_stale: bool,
    pub(crate) node_batches: HashMap<RetainedNodeId, u32>,
    container_batches: HashMap<RetainedParent, u32>,
    root_plan_fragments: HashMap<RetainedNodeId, RootPlanFragment>,
    root_fragment_owners: HashMap<RetainedNodeId, RetainedNodeId>,
    node_metadata: HashMap<RetainedNodeId, MaterializedNodeMetadata>,
}

static NEXT_PLAN_CACHE_KEY: AtomicU64 = AtomicU64::new(1);

fn next_plan_cache_key() -> u64 {
    (1 << 63) | NEXT_PLAN_CACHE_KEY.fetch_add(1, Ordering::Relaxed)
}
