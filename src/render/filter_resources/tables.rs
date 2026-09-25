//! Prepare quantized transfer, convolution and seeded turbulence data without a device.

use crate::shared::{
    execution::{ExecOp, ExecPlan},
    layer::{
        Layer,
        filter::{
            ComponentTransferTable, ConvolveMatrix, Filter, FilterPrimitiveKind,
            TURBULENCE_GRADIENT_LEN, TURBULENCE_TABLE_LEN, Turbulence, turbulence_lattice,
        },
    },
};

#[derive(Default)]
pub(crate) struct FilterTransferUpload {
    pub(crate) tables: Vec<u32>,
}

impl FilterTransferUpload {
    pub(crate) fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_transfers_for_ops(&plan.ops, &mut upload);
        upload
    }

    pub(crate) fn from_ops_and_filter(ops: &[ExecOp], filter: &Filter) -> Self {
        let mut upload = Self::default();
        collect_filter_transfers_for_ops(ops, &mut upload);
        collect_filter_transfer(filter, &mut upload);
        upload
    }

    fn push_table(&mut self, table: &ComponentTransferTable) {
        self.tables.extend_from_slice(table);
    }
}

#[derive(Default)]
pub(crate) struct FilterConvolveUpload {
    pub(crate) kernels: Vec<f32>,
}

impl FilterConvolveUpload {
    pub(crate) fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_convolves_for_ops(&plan.ops, &mut upload);
        upload
    }

    pub(crate) fn from_ops_and_filter(ops: &[ExecOp], filter: &Filter) -> Self {
        let mut upload = Self::default();
        collect_filter_convolves_for_ops(ops, &mut upload);
        collect_filter_convolve(filter, &mut upload);
        upload
    }

    fn push_matrix(&mut self, matrix: &ConvolveMatrix) {
        self.kernels.extend_from_slice(&matrix.data);
    }
}

#[derive(Default)]
pub(crate) struct FilterTurbulenceUpload {
    pub(crate) selectors: Vec<u32>,
    pub(crate) gradients: Vec<f32>,
}

impl FilterTurbulenceUpload {
    pub(crate) fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_turbulence_for_ops(&plan.ops, &mut upload);
        upload
    }

    pub(crate) fn from_ops_and_filter(ops: &[ExecOp], filter: &Filter) -> Self {
        let mut upload = Self::default();
        collect_filter_turbulence_for_ops(ops, &mut upload);
        collect_filter_turbulence(filter, &mut upload);
        upload
    }

    fn push_turbulence(&mut self, turbulence: &Turbulence) {
        let lattice = turbulence_lattice(turbulence.seed);
        debug_assert_eq!(lattice.selectors.len(), TURBULENCE_TABLE_LEN);
        debug_assert_eq!(lattice.gradients.len(), TURBULENCE_GRADIENT_LEN);
        self.selectors.extend_from_slice(&lattice.selectors);
        self.gradients.extend_from_slice(&lattice.gradients);
    }
}

fn collect_filter_transfers_for_ops(ops: &[ExecOp], upload: &mut FilterTransferUpload) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                layer, children, ..
            } => match layer {
                Layer::Filter { filter, .. } => {
                    collect_filter_transfers_for_ops(children, upload);
                    collect_filter_transfer(filter, upload);
                }
                Layer::Backdrop { filter, .. } => {
                    collect_filter_transfer(filter, upload);
                    collect_filter_transfers_for_ops(children, upload);
                }
                _ => collect_filter_transfers_for_ops(children, upload),
            },
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                collect_filter_transfers_for_ops(content, upload);
                collect_filter_transfers_for_ops(mask, upload);
            }
            _ => {}
        }
    }
}

fn collect_filter_convolves_for_ops(ops: &[ExecOp], upload: &mut FilterConvolveUpload) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                layer, children, ..
            } => match layer {
                Layer::Filter { filter, .. } => {
                    collect_filter_convolves_for_ops(children, upload);
                    collect_filter_convolve(filter, upload);
                }
                Layer::Backdrop { filter, .. } => {
                    collect_filter_convolve(filter, upload);
                    collect_filter_convolves_for_ops(children, upload);
                }
                _ => collect_filter_convolves_for_ops(children, upload),
            },
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                collect_filter_convolves_for_ops(content, upload);
                collect_filter_convolves_for_ops(mask, upload);
            }
            _ => {}
        }
    }
}

fn collect_filter_turbulence_for_ops(ops: &[ExecOp], upload: &mut FilterTurbulenceUpload) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                layer, children, ..
            } => match layer {
                Layer::Filter { filter, .. } => {
                    collect_filter_turbulence_for_ops(children, upload);
                    collect_filter_turbulence(filter, upload);
                }
                Layer::Backdrop { filter, .. } => {
                    collect_filter_turbulence(filter, upload);
                    collect_filter_turbulence_for_ops(children, upload);
                }
                _ => collect_filter_turbulence_for_ops(children, upload),
            },
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                collect_filter_turbulence_for_ops(content, upload);
                collect_filter_turbulence_for_ops(mask, upload);
            }
            _ => {}
        }
    }
}

fn collect_filter_transfer(filter: &Filter, upload: &mut FilterTransferUpload) {
    match filter {
        Filter::Chain { filters, .. } => {
            for filter in filters {
                collect_filter_transfer(filter, upload);
            }
        }
        Filter::Graph { primitives, .. } => {
            for primitive in primitives {
                if let FilterPrimitiveKind::Filter(filter) = &primitive.kind {
                    collect_filter_transfer(filter, upload);
                }
            }
        }
        Filter::ComponentTransfer(table) => upload.push_table(table),
        _ => {}
    }
}

fn collect_filter_convolve(filter: &Filter, upload: &mut FilterConvolveUpload) {
    match filter {
        Filter::Chain { filters, .. } => {
            for filter in filters {
                collect_filter_convolve(filter, upload);
            }
        }
        Filter::Graph { primitives, .. } => {
            for primitive in primitives {
                if let FilterPrimitiveKind::Filter(filter) = &primitive.kind {
                    collect_filter_convolve(filter, upload);
                }
            }
        }
        Filter::ConvolveMatrix(matrix) => upload.push_matrix(matrix),
        _ => {}
    }
}

fn collect_filter_turbulence(filter: &Filter, upload: &mut FilterTurbulenceUpload) {
    match filter {
        Filter::Chain { filters, .. } => {
            for filter in filters {
                collect_filter_turbulence(filter, upload);
            }
        }
        Filter::Graph { primitives, .. } => {
            for primitive in primitives {
                match &primitive.kind {
                    FilterPrimitiveKind::Filter(filter) => {
                        collect_filter_turbulence(filter, upload);
                    }
                    FilterPrimitiveKind::Turbulence(turbulence) => {
                        upload.push_turbulence(turbulence);
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}
