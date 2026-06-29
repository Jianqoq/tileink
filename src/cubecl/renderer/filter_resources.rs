use ::cubecl::{client::ComputeClient, prelude::Runtime};

use crate::{
    cubecl::{buffer::CubeBuffer, pipelines::filter::FilterPathResources},
    shared::{
        execution::{ExecOp, ExecPlan},
        layer::{
            Layer,
            filter::{
                ComponentTransferTable, ConvolveMatrix, Filter, FilterPrimitiveKind,
                TURBULENCE_GRADIENT_LEN, TURBULENCE_TABLE_LEN, Turbulence, turbulence_lattice,
            },
            region::Region,
        },
        path_flatten::PathFlatten,
    },
};

pub(super) struct FilterTransferBuffers {
    pub(crate) tables: CubeBuffer<u32>,
}

pub(super) struct FilterTurbulenceBuffers {
    pub(crate) selectors: CubeBuffer<u32>,
    pub(crate) gradients: CubeBuffer<f32>,
}

pub(super) struct FilterConvolveBuffers {
    pub(crate) kernels: CubeBuffer<f32>,
}

impl FilterConvolveBuffers {
    pub(super) fn new<R: Runtime>(client: &ComputeClient<R>) -> Self {
        Self {
            kernels: CubeBuffer::new(client, 0),
        }
    }

    pub(super) fn upload<R: Runtime>(
        &mut self,
        client: &ComputeClient<R>,
        upload: FilterConvolveUpload,
    ) {
        self.kernels.replace(client, &upload.kernels);
    }
}

#[derive(Default)]
pub(super) struct FilterConvolveUpload {
    kernels: Vec<f32>,
}

impl FilterConvolveUpload {
    pub(super) fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_convolves_for_ops(&plan.ops, &mut upload);
        upload
    }

    pub(super) fn from_ops_and_filter(ops: &[ExecOp], filter: &Filter) -> Self {
        let mut upload = Self::default();
        collect_filter_convolves_for_ops(ops, &mut upload);
        collect_filter_convolve(filter, &mut upload);
        upload
    }

    fn push_matrix(&mut self, matrix: &ConvolveMatrix) {
        self.kernels.extend_from_slice(&matrix.data);
    }
}

impl FilterTransferBuffers {
    pub(super) fn new<R: Runtime>(client: &ComputeClient<R>) -> Self {
        Self {
            tables: CubeBuffer::new(client, 0),
        }
    }

    pub(super) fn upload<R: Runtime>(
        &mut self,
        client: &ComputeClient<R>,
        upload: FilterTransferUpload,
    ) {
        self.tables.replace(client, &upload.tables);
    }
}

impl FilterTurbulenceBuffers {
    pub(super) fn new<R: Runtime>(client: &ComputeClient<R>) -> Self {
        Self {
            selectors: CubeBuffer::new(client, 0),
            gradients: CubeBuffer::new(client, 0),
        }
    }

    pub(super) fn upload<R: Runtime>(
        &mut self,
        client: &ComputeClient<R>,
        upload: FilterTurbulenceUpload,
    ) {
        self.selectors.replace(client, &upload.selectors);
        self.gradients.replace(client, &upload.gradients);
    }
}

#[derive(Default)]
pub(super) struct FilterTransferUpload {
    tables: Vec<u32>,
}

#[derive(Default)]
pub(super) struct FilterTurbulenceUpload {
    selectors: Vec<u32>,
    gradients: Vec<f32>,
}

impl FilterTransferUpload {
    pub(super) fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_transfers_for_ops(&plan.ops, &mut upload);
        upload
    }

    pub(super) fn from_ops_and_filter(ops: &[ExecOp], filter: &Filter) -> Self {
        let mut upload = Self::default();
        collect_filter_transfers_for_ops(ops, &mut upload);
        collect_filter_transfer(filter, &mut upload);
        upload
    }

    fn push_table(&mut self, table: &ComponentTransferTable) {
        self.tables.extend_from_slice(table);
    }
}

impl FilterTurbulenceUpload {
    pub(super) fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_turbulence_for_ops(&plan.ops, &mut upload);
        upload
    }

    pub(super) fn from_ops_and_filter(ops: &[ExecOp], filter: &Filter) -> Self {
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

pub(super) struct FilterPathBuffers {
    pub(crate) range_starts: CubeBuffer<u32>,
    pub(crate) range_ends: CubeBuffer<u32>,
    pub(crate) p0x: CubeBuffer<i32>,
    pub(crate) p0y: CubeBuffer<i32>,
    pub(crate) p1x: CubeBuffer<i32>,
    pub(crate) p1y: CubeBuffer<i32>,
}

impl FilterPathBuffers {
    pub(super) fn new<R: Runtime>(client: &ComputeClient<R>) -> Self {
        Self {
            range_starts: CubeBuffer::new(client, 0),
            range_ends: CubeBuffer::new(client, 0),
            p0x: CubeBuffer::new(client, 0),
            p0y: CubeBuffer::new(client, 0),
            p1x: CubeBuffer::new(client, 0),
            p1y: CubeBuffer::new(client, 0),
        }
    }

    pub(super) fn upload<R: Runtime>(
        &mut self,
        client: &ComputeClient<R>,
        upload: FilterPathUpload,
    ) {
        self.range_starts.replace(client, &upload.range_starts);
        self.range_ends.replace(client, &upload.range_ends);
        self.p0x.replace(client, &upload.p0x);
        self.p0y.replace(client, &upload.p0y);
        self.p1x.replace(client, &upload.p1x);
        self.p1y.replace(client, &upload.p1y);
    }

    pub(super) fn resources(&self) -> FilterPathResources<'_> {
        FilterPathResources {
            range_starts: &self.range_starts,
            range_ends: &self.range_ends,
            p0x: &self.p0x,
            p0y: &self.p0y,
            p1x: &self.p1x,
            p1y: &self.p1y,
        }
    }
}

#[derive(Default)]
pub(super) struct FilterPathUpload {
    range_starts: Vec<u32>,
    range_ends: Vec<u32>,
    p0x: Vec<i32>,
    p0y: Vec<i32>,
    p1x: Vec<i32>,
    p1y: Vec<i32>,
}

impl FilterPathUpload {
    pub(super) fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_paths_for_ops(&plan.ops, &mut upload);
        upload
    }

    fn push_region(&mut self, region: &Region) {
        let Region::Path {
            path,
            transform,
            tolerance,
        } = region
        else {
            return;
        };

        let start = self.p0x.len() as u32;
        let path = *transform * path;
        let mut tile_count = 0;
        let mut lines = Vec::new();
        PathFlatten::new(
            &path,
            *tolerance as f32,
            self.range_starts.len() as u32,
            &mut tile_count,
        )
        .flatten(&mut lines);
        self.p0x.extend(
            lines
                .iter()
                .map(|line| encode_filter_path_coord(line.p0[0])),
        );
        self.p0y.extend(
            lines
                .iter()
                .map(|line| encode_filter_path_coord(line.p0[1])),
        );
        self.p1x.extend(
            lines
                .iter()
                .map(|line| encode_filter_path_coord(line.p1[0])),
        );
        self.p1y.extend(
            lines
                .iter()
                .map(|line| encode_filter_path_coord(line.p1[1])),
        );
        self.range_starts.push(start);
        self.range_ends.push(self.p0x.len() as u32);
    }
}

fn encode_filter_path_coord(value: f32) -> i32 {
    (value * 256.0)
        .round()
        .clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

fn collect_filter_paths_for_ops(ops: &[ExecOp], upload: &mut FilterPathUpload) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                layer, children, ..
            } => match layer {
                Layer::Filter { sample_region, .. } | Layer::Backdrop { sample_region, .. } => {
                    upload.push_region(sample_region);
                    collect_filter_paths_for_ops(children, upload);
                }
                _ => collect_filter_paths_for_ops(children, upload),
            },
            ExecOp::OffscreenMaskLayer {
                layer,
                content,
                mask,
                ..
            } => {
                upload.push_region(&layer.region);
                collect_filter_paths_for_ops(content, upload);
                collect_filter_paths_for_ops(mask, upload);
            }
            _ => {}
        }
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
                        collect_filter_turbulence(filter, upload)
                    }
                    FilterPrimitiveKind::Turbulence(turbulence) => {
                        upload.push_turbulence(turbulence)
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}
