use crate::shared::{
    execution::ExecOp,
    layer::{
        Layer,
        filter::{ConvolveMatrix, Filter, FilterPrimitiveKind},
        region::Region,
    },
};

#[derive(Default)]
pub(super) struct FilterCursors {
    brush: usize,
    convolve: usize,
    path: usize,
    transfer: usize,
    turbulence: usize,
}

impl FilterCursors {
    pub(super) fn next_brush_index(&mut self) -> u32 {
        let index = self.brush as u32;
        self.brush += 1;
        index
    }

    pub(super) fn next_transfer_index(&mut self) -> u32 {
        let index = self.transfer as u32;
        self.transfer += 1;
        index
    }

    pub(super) fn next_convolve_offset(&mut self, matrix: &ConvolveMatrix) -> u32 {
        let offset = self.convolve as u32;
        self.convolve += matrix.data.len();
        offset
    }

    pub(super) fn next_turbulence_index(&mut self) -> u32 {
        let index = self.turbulence as u32;
        self.turbulence += 1;
        index
    }

    pub(super) fn next_path_index(&mut self, region: &Region) -> Option<u32> {
        if matches!(region, Region::Path { .. }) {
            let index = self.path as u32;
            self.path += 1;
            Some(index)
        } else {
            None
        }
    }

    pub(super) fn advance_filter_layer(
        &mut self,
        sample_region: &Region,
        children: &[ExecOp],
        filter: &Filter,
    ) {
        self.next_path_index(sample_region);
        self.advance_ops(children);
        self.advance_filter(filter);
    }

    pub(super) fn advance_ops(&mut self, ops: &[ExecOp]) {
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

    pub(super) fn advance_filter(&mut self, filter: &Filter) {
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
                        FilterPrimitiveKind::Image { .. } => {
                            self.next_brush_index();
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
            Filter::Flood { .. } | Filter::DropShadow { .. } => {
                self.next_brush_index();
            }
            _ => {}
        }
    }
}
