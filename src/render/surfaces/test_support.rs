//! Recording adapter for offscreen layer tests. It models logical pool ownership
//! and injects failures independently from GPU/API implementations.
use crate::canvas::{RetainedFrame, RetainedNodeKind, RetainedNodeState, RetainedSurfaceId};
use crate::render::{
    draw_batches::DrawBatchAdapter,
    filter_resources::cursors::FilterCursors,
    incremental::{IncrementalRenderConfig, IncrementalRenderStats},
    operations::{Masked, Offscreen, OperationAdapter},
    output::RenderTargetId,
    retained::RetainedRenderState,
    retained_surfaces::{RetainedSurfaceKind, SurfaceAllocation},
};
use crate::retained_scene::{NodeGeneration, RetainedNodeId};
use crate::shared::{
    bounds::Bounds,
    execution::{ExecOp, ExecPlan},
    layer::{
        Layer,
        filter::{COMPONENT_TRANSFER_TABLE_LEN, Filter},
        mask::MaskKind,
        region::Region,
    },
};
use crate::{Canvas, Radius};
use peniko::{Color, kurbo::Rect};
use std::{ops::Range, rc::Rc};
#[derive(Debug, PartialEq)]
pub(crate) enum MaskCall {
    Coverage(RenderTargetId, RenderTargetId, Bounds, MaskKind),
    Region(RenderTargetId, Option<u32>, Bounds),
    Apply(RenderTargetId, RenderTargetId, Bounds),
}
#[derive(Debug, PartialEq)]
pub(crate) enum Event {
    Acquire(RenderTargetId),
    Install(RenderTargetId, u32),
    Release(RenderTargetId),
    Clear(RenderTargetId),
    ClearRegion(RenderTargetId, Bounds),
    Draw(RenderTargetId),
    Filter,
    BeginRootBatch,
    SuspendBackdrop,
    RestoreBackdrop,
    BackdropPass(RenderTargetId, RenderTargetId, Option<Bounds>, bool),
    DirectBackdrop,
    RectBackdrop(u32),
    Copy(RenderTargetId, RenderTargetId, Bounds),
    FilterPass(RenderTargetId, Bounds),
    OutputDamage,
    BeginFilter((u32, u32), (i32, i32), bool),
    EndFilter,
    SourceWork,
    ScanFilter,
    Opacity(f32),
    Mask(u32),
    Coverage(MaskKind),
    Region(Option<u32>),
    Apply,
    Composite(Option<peniko::BlendMode>),
    Cached(u32, Option<u32>, Option<peniko::BlendMode>),
}

pub(crate) struct Allocation(pub(crate) u32);

impl SurfaceAllocation for Allocation {
    fn byte_len(&self) -> u64 {
        4096
    }
}

pub(crate) struct Adapter {
    pub(crate) filter_context: Option<((u32, u32), (i32, i32))>,
    pub(crate) filter_ends: usize,
    pub(crate) candidate_queries: std::cell::Cell<usize>,
    pub(crate) candidate_draws: Vec<u32>,
    pub(crate) retained: RetainedRenderState<Allocation>,
    pub(crate) targets: Vec<Option<Allocation>>,
    pub(crate) events: Vec<Event>,
    pub(crate) fail: Option<&'static str>,
    pub(crate) skip_failures: usize,
    pub(crate) scratch_limit: usize,
    pub(crate) expected_stack: Range<usize>,
    kind: RetainedSurfaceKind,
    last_batch: u32,
    pub(crate) draw_batches: Vec<(u32, RenderTargetId)>,
    pub(crate) filters: Vec<u32>,
    pub(crate) filter_transfer_indices: Vec<u32>,
    pub(crate) filter_path_indices: Vec<Option<u32>>,
    pub(crate) filter_placements:
        Vec<(RenderTargetId, u32, crate::render::filters::FilterPlacement)>,
    pub(crate) mask_calls: Vec<MaskCall>,
    pub(crate) composites: Vec<(RenderTargetId, RenderTargetId, RenderTargetId, Bounds)>,
}

impl Adapter {
    pub(crate) fn new(canvas: &Canvas, kind: RetainedSurfaceKind) -> Self {
        let mut retained = RetainedRenderState::new(IncrementalRenderConfig::default());
        retained.begin_frame(Some(frame(1)), canvas, false);
        Self {
            filter_context: None,
            filter_ends: 0,
            candidate_queries: std::cell::Cell::new(0),
            candidate_draws: (0..canvas.draw_records.len() as u32).collect(),
            retained,
            kind,
            last_batch: 0,
            draw_batches: Vec::new(),
            filters: Vec::new(),
            filter_transfer_indices: Vec::new(),
            filter_path_indices: Vec::new(),
            filter_placements: Vec::new(),
            mask_calls: Vec::new(),
            composites: Vec::new(),
            targets: Vec::new(),
            events: Vec::new(),
            fail: None,
            skip_failures: 0,
            scratch_limit: 8,
            expected_stack: 2..4,
        }
    }
    pub(crate) fn record(&mut self, event: Event, stage: &'static str) -> Result<(), &'static str> {
        self.events.push(event);
        if self.fail == Some(stage) && self.skip_failures == 0 {
            Err(stage)
        } else {
            if self.fail == Some(stage) {
                self.skip_failures -= 1;
            }
            Ok(())
        }
    }
    pub(crate) fn cached(&mut self) {
        let meta = self
            .retained
            .surface_meta(id(), self.kind, (32, 32), (4, 8), Bounds::canvas(32, 32))
            .unwrap();
        self.retained.cache_surface(
            Some(id()),
            Some(meta),
            Allocation(20),
            Some(Allocation(30)),
            None,
        );
    }
    pub(crate) fn cached_ids(&mut self, revision: u64) -> (u32, u32) {
        let meta = crate::render::retained_surfaces::RetainedSurfaceMeta {
            revision: NodeGeneration::new(revision),
            kind: self.kind,
            size: (32, 32),
            origin: (4, 8),
            bounds: Bounds::canvas(32, 32),
        };
        let (_, surface) = self
            .retained
            .take_matching_surface(Some(id()), Some(meta))
            .unwrap();
        (surface.primary.0, surface.secondary.unwrap().0)
    }
}

impl DrawBatchAdapter for Adapter {
    type Error = &'static str;
    fn stats_mut(&mut self) -> &mut IncrementalRenderStats {
        self.retained.stats_mut()
    }
    fn begin_root_batch(&mut self) -> Result<(), Self::Error> {
        assert_eq!(
            self.kind,
            RetainedSurfaceKind::Backdrop,
            "only backdrop foreground may use this fixture's root submission budget"
        );
        self.record(Event::BeginRootBatch, "root-batch")
    }
    fn coarse(&mut self, batches: Range<u32>, _: Range<u32>) -> Result<(), Self::Error> {
        self.last_batch = batches.start;
        Ok(())
    }
    fn fine(&mut self, target: RenderTargetId) -> Result<(), Self::Error> {
        self.draw_batches.push((self.last_batch, target));
        self.record(Event::Draw(target), "children")
    }
}

impl OperationAdapter for Adapter {
    fn offscreen(
        &mut self,
        _: &Canvas,
        _: &ExecPlan,
        op: Offscreen<'_>,
        _: RenderTargetId,
        cursors: &mut FilterCursors,
    ) -> Result<(), Self::Error> {
        let Layer::Filter {
            filter,
            sample_region,
        } = op.layer
        else {
            panic!("expected the child filter")
        };
        let Filter::ComponentTransfer(table) = filter else {
            panic!("expected transfer fixture");
        };
        let mut resource_cursors = cursors.clone();
        self.filter_path_indices
            .push(resource_cursors.next_path_index(sample_region));
        resource_cursors.advance_ops(op.children);
        self.filter_transfer_indices
            .push(resource_cursors.next_transfer_index());
        self.filters.push(table[0]);
        self.record(Event::Filter, "filter")?;
        cursors.advance_filter_layer(sample_region, op.children, filter);
        Ok(())
    }
    fn mask(
        &mut self,
        _: &Canvas,
        _: &ExecPlan,
        _: Masked<'_>,
        _: RenderTargetId,
        _: &mut FilterCursors,
    ) -> Result<(), Self::Error> {
        panic!("unexpected mask operation")
    }
}

impl crate::render::surfaces::SurfaceAdapter for Adapter {
    type Surface = Allocation;
    fn size(&self) -> (u32, u32) {
        self.filter_context.map_or((32, 32), |context| context.0)
    }
    fn origin(&self) -> (i32, i32) {
        self.filter_context.map_or((4, 8), |context| context.1)
    }
    fn retained(&self) -> &RetainedRenderState<Allocation> {
        &self.retained
    }
    fn retained_mut(&mut self) -> &mut RetainedRenderState<Allocation> {
        &mut self.retained
    }
    fn acquire_scratch(&mut self) -> Result<RenderTargetId, Self::Error> {
        let slot = self
            .targets
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.targets.len());
        if slot >= self.scratch_limit {
            return Err("scratch");
        }
        let allocation = Some(Allocation(100 + slot as u32));
        if slot == self.targets.len() {
            self.targets.push(allocation);
        } else {
            self.targets[slot] = allocation;
        }
        let target = RenderTargetId::Scratch(slot);
        self.events.push(Event::Acquire(target));
        Ok(target)
    }
    fn install_scratch(&mut self, target: RenderTargetId, surface: Allocation) {
        self.events.push(Event::Install(target, surface.0));
        let RenderTargetId::Scratch(slot) = target else {
            panic!("scratch only")
        };
        self.targets[slot] = Some(surface);
    }
    fn take_scratch(&mut self, target: RenderTargetId) -> Option<Allocation> {
        let RenderTargetId::Scratch(slot) = target else {
            panic!("scratch only")
        };
        self.targets[slot].take()
    }
    fn release_scratch(&mut self, target: RenderTargetId) {
        self.events.push(Event::Release(target));
        assert!(
            self.take_scratch(target).is_some(),
            "release must own an occupied scratch slot"
        );
    }
    fn clear_target(&mut self, target: RenderTargetId) -> Result<(), Self::Error> {
        self.record(Event::Clear(target), "clear")
    }
    fn clear_region(&mut self, target: RenderTargetId, bounds: Bounds) -> Result<(), Self::Error> {
        self.record(Event::ClearRegion(target, bounds), "clear")
    }
    fn composite_cached(
        &mut self,
        _: RenderTargetId,
        source: &Allocation,
        mask: Option<&Allocation>,
        _: Bounds,
        stack: Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> Result<(), Self::Error> {
        assert_eq!(stack, self.expected_stack);
        self.record(
            Event::Cached(source.0, mask.map(|mask| mask.0), blend),
            "composite",
        )
    }
    fn composite_targets(
        &mut self,
        target: RenderTargetId,
        source: RenderTargetId,
        mask: RenderTargetId,
        bounds: Bounds,
        stack: Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> Result<(), Self::Error> {
        assert_eq!(stack, self.expected_stack);
        self.composites.push((target, source, mask, bounds));
        self.record(Event::Composite(blend), "composite")
    }
}

pub(crate) fn id() -> RetainedSurfaceId {
    RetainedSurfaceId {
        node: RetainedNodeId::for_owner(2),
        slot: 0,
    }
}

pub(crate) fn frame(revision: u64) -> RetainedFrame {
    let node = RetainedNodeState {
        id: id().node,
        revision: NodeGeneration::new(revision),
        bounds: Bounds::canvas(32, 32),
        order: 0,
        kind: RetainedNodeKind::Scene,
        placement_bits: None,
    };
    RetainedFrame {
        root: RetainedNodeId::for_owner(1),
        logical_size: (32, 32),
        physical_size: (32, 32),
        scale_bits: 1.0f32.to_bits(),
        nodes: vec![node].into(),
        node_index: Rc::new([(node.id, 0)].into_iter().collect()),
        state_pages: Rc::new(Default::default()),
        invalidated_bounds: Vec::new(),
        invalidate_all: false,
        incremental_complete: true,
        version: None,
        delta: None,
        damage_history: crate::canvas::damage_history::DamageHistory::default(),
        dependency_free: true,
        requires_damage_propagation: false,
    }
}

pub(crate) fn fixture() -> (Canvas, ExecPlan) {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO, Color::WHITE);
    let ops = vec![
        ExecOp::DrawBatch {
            draws: Rc::new(vec![0]),
            batch_id: 0,
            owners: Rc::default(),
            layer_stack: 0..0,
        },
        ExecOp::OffscreenLayer {
            retained_id: None,
            draw: 0,
            layer: Layer::Filter {
                filter: Filter::ComponentTransfer(Box::new([11; COMPONENT_TRANSFER_TABLE_LEN])),
                sample_region: Region::Rect {
                    rect: Rect::new(0.0, 0.0, 32.0, 32.0),
                    radius: Radius::ZERO,
                },
            },
            outer_stack: 0..0,
            children: Vec::new(),
        },
    ];
    let plan = ExecPlan {
        ops,
        layer_stack_data: Vec::new(),
        draw_order: Rc::default(),
        draw_batch_ids: Rc::default(),
        retained_batch_ids: Default::default(),
        layer_stack_locations: Default::default(),
        direct_root_batch_ops: None,
    };
    (canvas, plan)
}

impl crate::render::filter_pass::FilterPassAdapter for Adapter {
    fn copy_filter_region(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
    ) -> Result<(), Self::Error> {
        self.record(Event::Copy(source, target, bounds), "copy")
    }
    fn apply_filter_pass(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        cursors: &mut FilterCursors,
    ) -> Result<(), Self::Error> {
        self.record(Event::FilterPass(target, bounds), "apply")?;
        if let Filter::ComponentTransfer(table) = filter {
            self.filter_transfer_indices
                .push(cursors.clone().next_transfer_index());
            self.filters.push(table[0]);
        }
        cursors.advance_filter(filter);
        Ok(())
    }
    fn prepare_filter_output_work(&mut self) -> Result<(), Self::Error> {
        self.record(Event::OutputDamage, "output-work")
    }
}
