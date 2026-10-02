//! Execution indices mirror the upload traversal, including skipped offscreen work.

use crate::shared::{
    brush::encoded_brush_word_len,
    execution::ExecOp,
    layer::{
        Layer,
        filter::{ConvolveMatrix, Filter, FilterPrimitiveKind},
        region::Region,
    },
};

#[derive(Clone, Default)]
pub(crate) struct FilterCursors {
    transfer: usize,
    brush_offset: usize,
    convolve: usize,
    turbulence: usize,
    path: usize,
}

impl FilterCursors {
    pub(crate) fn next_transfer_index(&mut self) -> u32 {
        let index = self.transfer as u32;
        self.transfer += 1;
        index
    }

    pub(crate) fn next_brush_offset(&mut self, brush: &crate::shared::brush::Brush) -> u32 {
        let offset = self.brush_offset as u32;
        self.brush_offset += encoded_brush_word_len(brush);
        offset
    }

    pub(crate) fn next_convolve_offset(&mut self, matrix: &ConvolveMatrix) -> u32 {
        let offset = self.convolve as u32;
        self.convolve += matrix.data.len();
        offset
    }

    pub(crate) fn next_turbulence_index(&mut self) -> u32 {
        let index = self.turbulence as u32;
        self.turbulence += 1;
        index
    }

    pub(crate) fn next_path_index(&mut self, region: &Region) -> Option<u32> {
        if !matches!(region, Region::Path { .. }) {
            return None;
        }
        let index = self.path as u32;
        self.path += 1;
        Some(index)
    }

    pub(crate) fn advance_filter_layer(
        &mut self,
        sample_region: &Region,
        children: &[ExecOp],
        filter: &Filter,
    ) {
        self.next_path_index(sample_region);
        self.advance_ops(children);
        self.advance_filter(filter);
    }

    pub(crate) fn advance_ops(&mut self, ops: &[ExecOp]) {
        for op in ops {
            match op {
                ExecOp::OffscreenLayer {
                    layer, children, ..
                } => match layer {
                    Layer::Filter {
                        filter,
                        sample_region,
                    } => self.advance_filter_layer(sample_region, children, filter),
                    Layer::Backdrop {
                        filter,
                        sample_region,
                    } => {
                        self.next_path_index(sample_region);
                        self.advance_filter(filter);
                        self.advance_ops(children);
                    }
                    _ => self.advance_ops(children),
                },
                ExecOp::OffscreenMaskLayer {
                    layer,
                    content,
                    mask,
                    ..
                } => {
                    self.next_path_index(&layer.region);
                    self.advance_ops(content);
                    self.advance_ops(mask);
                }
                _ => {}
            }
        }
    }

    pub(crate) fn advance_filter(&mut self, filter: &Filter) {
        match filter {
            Filter::Chain { filters, .. } => {
                for filter in filters {
                    self.advance_filter(filter);
                }
            }
            Filter::Graph { primitives, .. } => {
                for primitive in primitives {
                    match &primitive.kind {
                        FilterPrimitiveKind::Filter(filter) => self.advance_filter(filter),
                        FilterPrimitiveKind::Image { brush } => {
                            self.next_brush_offset(brush);
                        }
                        FilterPrimitiveKind::Turbulence(_) => {
                            self.next_turbulence_index();
                        }
                        _ => {}
                    }
                }
            }
            Filter::ComponentTransfer(_) => {
                self.next_transfer_index();
            }
            Filter::ConvolveMatrix(matrix) => {
                self.next_convolve_offset(matrix);
            }
            Filter::Flood { brush } | Filter::DropShadow { brush, .. } => {
                self.next_brush_offset(brush);
            }
            _ => {}
        }
    }
}
