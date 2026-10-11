//! Collect filter payloads once in execution order. Transfer tables borrow their
//! original words; seeded lattices and brush records go directly to native upload.
use crate::shared::{
    execution::{ExecOp, OffscreenLayer},
    gpu_brush::GpuBrushUpload,
    image_resource::GpuImageResourceUpload,
    layer::filter::{
        ComponentTransferTable, Filter, FilterPrimitiveKind, TurbulenceLattice, turbulence_lattice,
    },
};

#[derive(Default)]
pub(crate) struct FilterResourceUpload<'a> {
    pub(crate) transfers: Vec<&'a ComponentTransferTable>,
    pub(crate) convolves: Vec<f32>,
    pub(crate) turbulence: Vec<TurbulenceLattice>,
    pub(crate) brushes: GpuBrushUpload,
}

impl<'a> FilterResourceUpload<'a> {
    pub(crate) fn new(
        ops: &'a [ExecOp],
        filter: Option<&'a Filter>,
        images: Option<&GpuImageResourceUpload>,
    ) -> Self {
        let mut upload = Self::default();
        upload.collect_ops(ops, images);
        if let Some(filter) = filter {
            upload.collect_filter(filter, images);
        }
        upload
    }

    fn collect_ops(&mut self, ops: &'a [ExecOp], images: Option<&GpuImageResourceUpload>) {
        for op in ops {
            match op {
                ExecOp::OffscreenLayer {
                    layer, children, ..
                } => match layer {
                    OffscreenLayer::Filter { filter, .. } => {
                        self.collect_ops(children, images);
                        self.collect_filter(filter, images);
                    }
                    OffscreenLayer::Backdrop { filter, .. } => {
                        self.collect_filter(filter, images);
                        self.collect_ops(children, images);
                    }
                    OffscreenLayer::Isolate { .. }
                    | OffscreenLayer::Opacity { .. }
                    | OffscreenLayer::Blend { .. } => self.collect_ops(children, images),
                },
                ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                    self.collect_ops(content, images);
                    self.collect_ops(mask, images);
                }
                _ => {}
            }
        }
    }

    fn collect_filter(&mut self, filter: &'a Filter, images: Option<&GpuImageResourceUpload>) {
        match filter {
            Filter::Chain { filters, .. } => {
                for filter in filters {
                    self.collect_filter(filter, images);
                }
            }
            Filter::Graph { primitives, .. } => {
                for primitive in primitives {
                    match &primitive.kind {
                        FilterPrimitiveKind::Filter(filter) => self.collect_filter(filter, images),
                        FilterPrimitiveKind::Turbulence(turbulence) => {
                            self.turbulence.push(turbulence_lattice(turbulence.seed));
                        }
                        FilterPrimitiveKind::Image { brush } => {
                            self.brushes.push_brush_with_resources(brush, images);
                        }
                        _ => {}
                    }
                }
            }
            Filter::ComponentTransfer(table) => self.transfers.push(table),
            Filter::ConvolveMatrix(matrix) => self.convolves.extend_from_slice(&matrix.data),
            Filter::Flood { brush } | Filter::DropShadow { brush, .. } => {
                self.brushes.push_brush_with_resources(brush, images);
            }
            _ => {}
        }
    }
}
