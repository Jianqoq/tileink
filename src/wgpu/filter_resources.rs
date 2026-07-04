use crate::shared::{
    execution::{ExecOp, ExecPlan},
    gpu_brush::GpuBrushUpload,
    image_resource::GpuImageResourceUpload,
    layer::{
        Layer,
        filter::{
            ComponentTransferTable, ConvolveMatrix, Filter, FilterPrimitiveKind,
            TURBULENCE_GRADIENT_LEN, TURBULENCE_TABLE_LEN, Turbulence, turbulence_lattice,
        },
        region::Region,
    },
    path_flatten::PathFlatten,
};

use super::buffer::WgpuBuffer;

pub(super) struct WgpuFilterTransferBuffers {
    pub(super) tables: WgpuBuffer,
}

pub(super) struct WgpuFilterBrushBuffers {
    pub(super) data: WgpuBuffer,
    pub(super) params: WgpuBuffer,
    pub(super) payloads: WgpuBuffer,
}

pub(super) struct WgpuFilterConvolveBuffers {
    pub(super) kernels: WgpuBuffer,
}

pub(super) struct WgpuFilterTurbulenceBuffers {
    pub(super) selectors: WgpuBuffer,
    pub(super) gradients: WgpuBuffer,
}

pub(super) struct WgpuFilterPathBuffers {
    pub(super) range_starts: WgpuBuffer,
    pub(super) range_ends: WgpuBuffer,
    pub(super) p0x: WgpuBuffer,
    pub(super) p0y: WgpuBuffer,
    pub(super) p1x: WgpuBuffer,
    pub(super) p1y: WgpuBuffer,
}

impl WgpuFilterTransferBuffers {
    pub(super) fn new(device: &::wgpu::Device) -> Self {
        Self {
            tables: WgpuBuffer::new(device, "tileink wgpu filter transfer tables"),
        }
    }

    pub(super) fn upload(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        plan: &ExecPlan,
    ) {
        let upload = FilterTransferUpload::from_plan(plan);
        self.upload_tables(device, queue, upload);
    }

    pub(super) fn upload_for_ops_and_filter(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        ops: &[ExecOp],
        filter: &Filter,
    ) {
        let upload = FilterTransferUpload::from_ops_and_filter(ops, filter);
        self.upload_tables(device, queue, upload);
    }

    fn upload_tables(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        upload: FilterTransferUpload,
    ) {
        self.tables.upload(
            device,
            queue,
            "tileink wgpu filter transfer tables",
            &upload.tables,
        );
    }
}

impl WgpuFilterBrushBuffers {
    pub(super) fn new(device: &::wgpu::Device) -> Self {
        Self {
            data: WgpuBuffer::new(device, "tileink wgpu filter brush data"),
            params: WgpuBuffer::new(device, "tileink wgpu filter brush params"),
            payloads: WgpuBuffer::new(device, "tileink wgpu filter brush payloads"),
        }
    }

    pub(super) fn upload(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        plan: &ExecPlan,
        image_resources: Option<&GpuImageResourceUpload>,
    ) {
        let upload = GpuBrushUpload::from_filter_plan_with_resources(&plan.ops, image_resources);
        self.upload_brushes(device, queue, upload);
    }

    pub(super) fn upload_for_ops_and_filter(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        ops: &[ExecOp],
        filter: &Filter,
        image_resources: Option<&GpuImageResourceUpload>,
    ) {
        let upload =
            GpuBrushUpload::from_filter_ops_and_filter_with_resources(ops, filter, image_resources);
        self.upload_brushes(device, queue, upload);
    }

    fn upload_brushes(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        upload: GpuBrushUpload,
    ) {
        self.data.upload(
            device,
            queue,
            "tileink wgpu filter brush data",
            &upload.data,
        );
        self.params.upload(
            device,
            queue,
            "tileink wgpu filter brush params",
            &upload.params,
        );
        self.payloads.upload(
            device,
            queue,
            "tileink wgpu filter brush payloads",
            &upload.payloads,
        );
    }
}

impl WgpuFilterConvolveBuffers {
    pub(super) fn new(device: &::wgpu::Device) -> Self {
        Self {
            kernels: WgpuBuffer::new(device, "tileink wgpu filter convolve kernels"),
        }
    }

    pub(super) fn upload(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        plan: &ExecPlan,
    ) {
        let upload = FilterConvolveUpload::from_plan(plan);
        self.upload_kernels(device, queue, upload);
    }

    pub(super) fn upload_for_ops_and_filter(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        ops: &[ExecOp],
        filter: &Filter,
    ) {
        let upload = FilterConvolveUpload::from_ops_and_filter(ops, filter);
        self.upload_kernels(device, queue, upload);
    }

    fn upload_kernels(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        upload: FilterConvolveUpload,
    ) {
        self.kernels.upload(
            device,
            queue,
            "tileink wgpu filter convolve kernels",
            &upload.kernels,
        );
    }
}

impl WgpuFilterTurbulenceBuffers {
    pub(super) fn new(device: &::wgpu::Device) -> Self {
        Self {
            selectors: WgpuBuffer::new(device, "tileink wgpu filter turbulence selectors"),
            gradients: WgpuBuffer::new(device, "tileink wgpu filter turbulence gradients"),
        }
    }

    pub(super) fn upload(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        plan: &ExecPlan,
    ) {
        let upload = FilterTurbulenceUpload::from_plan(plan);
        self.upload_tables(device, queue, upload);
    }

    pub(super) fn upload_for_ops_and_filter(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        ops: &[ExecOp],
        filter: &Filter,
    ) {
        let upload = FilterTurbulenceUpload::from_ops_and_filter(ops, filter);
        self.upload_tables(device, queue, upload);
    }

    fn upload_tables(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        upload: FilterTurbulenceUpload,
    ) {
        self.selectors.upload(
            device,
            queue,
            "tileink wgpu filter turbulence selectors",
            &upload.selectors,
        );
        self.gradients.upload(
            device,
            queue,
            "tileink wgpu filter turbulence gradients",
            &upload.gradients,
        );
    }
}

impl WgpuFilterPathBuffers {
    pub(super) fn new(device: &::wgpu::Device) -> Self {
        Self {
            range_starts: WgpuBuffer::new(device, "tileink wgpu filter path range starts"),
            range_ends: WgpuBuffer::new(device, "tileink wgpu filter path range ends"),
            p0x: WgpuBuffer::new(device, "tileink wgpu filter path p0x"),
            p0y: WgpuBuffer::new(device, "tileink wgpu filter path p0y"),
            p1x: WgpuBuffer::new(device, "tileink wgpu filter path p1x"),
            p1y: WgpuBuffer::new(device, "tileink wgpu filter path p1y"),
        }
    }

    pub(super) fn upload(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        plan: &ExecPlan,
    ) {
        let upload = FilterPathUpload::from_plan(plan);
        self.range_starts.upload(
            device,
            queue,
            "tileink wgpu filter path range starts",
            &upload.range_starts,
        );
        self.range_ends.upload(
            device,
            queue,
            "tileink wgpu filter path range ends",
            &upload.range_ends,
        );
        self.p0x
            .upload(device, queue, "tileink wgpu filter path p0x", &upload.p0x);
        self.p0y
            .upload(device, queue, "tileink wgpu filter path p0y", &upload.p0y);
        self.p1x
            .upload(device, queue, "tileink wgpu filter path p1x", &upload.p1x);
        self.p1y
            .upload(device, queue, "tileink wgpu filter path p1y", &upload.p1y);
    }
}

#[derive(Default)]
pub(super) struct WgpuFilterCursors {
    transfer: usize,
    brush: usize,
    convolve: usize,
    turbulence: usize,
    path: usize,
}

impl WgpuFilterCursors {
    pub(super) fn next_transfer_index(&mut self) -> u32 {
        let index = self.transfer as u32;
        self.transfer += 1;
        index
    }

    pub(super) fn next_brush_index(&mut self) -> u32 {
        let index = self.brush as u32;
        self.brush += 1;
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
        if !matches!(region, Region::Path { .. }) {
            return None;
        }
        let index = self.path as u32;
        self.path += 1;
        Some(index)
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

#[derive(Default)]
struct FilterTransferUpload {
    tables: Vec<u32>,
}

#[derive(Default)]
struct FilterConvolveUpload {
    kernels: Vec<f32>,
}

#[derive(Default)]
struct FilterTurbulenceUpload {
    selectors: Vec<u32>,
    gradients: Vec<f32>,
}

#[derive(Default)]
struct FilterPathUpload {
    range_starts: Vec<u32>,
    range_ends: Vec<u32>,
    p0x: Vec<i32>,
    p0y: Vec<i32>,
    p1x: Vec<i32>,
    p1y: Vec<i32>,
}

impl FilterTransferUpload {
    fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_transfers_for_ops(&plan.ops, &mut upload);
        upload
    }

    fn from_ops_and_filter(ops: &[ExecOp], filter: &Filter) -> Self {
        let mut upload = Self::default();
        collect_filter_transfers_for_ops(ops, &mut upload);
        collect_filter_transfer(filter, &mut upload);
        upload
    }

    fn push_table(&mut self, table: &ComponentTransferTable) {
        self.tables.extend_from_slice(table);
    }
}

impl FilterConvolveUpload {
    fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_convolves_for_ops(&plan.ops, &mut upload);
        upload
    }

    fn from_ops_and_filter(ops: &[ExecOp], filter: &Filter) -> Self {
        let mut upload = Self::default();
        collect_filter_convolves_for_ops(ops, &mut upload);
        collect_filter_convolve(filter, &mut upload);
        upload
    }

    fn push_matrix(&mut self, matrix: &ConvolveMatrix) {
        self.kernels.extend_from_slice(&matrix.data);
    }
}

impl FilterTurbulenceUpload {
    fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_turbulence_for_ops(&plan.ops, &mut upload);
        upload
    }

    fn from_ops_and_filter(ops: &[ExecOp], filter: &Filter) -> Self {
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

impl FilterPathUpload {
    fn from_plan(plan: &ExecPlan) -> Self {
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
        let mut lines = Vec::new();
        PathFlatten::new(&path, *tolerance as f32, self.range_starts.len() as u32)
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
