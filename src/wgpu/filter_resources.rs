use super::buffer::WgpuBuffer;
use crate::{
    render::filter_resources::{
        paths::FilterPathUpload,
        tables::{FilterConvolveUpload, FilterTransferUpload, FilterTurbulenceUpload},
    },
    shared::{
        execution::{ExecOp, ExecPlan},
        gpu_brush::GpuBrushUpload,
        image_resource::GpuImageResourceUpload,
        layer::filter::Filter,
    },
};

pub(super) struct WgpuFilterTransferBuffers {
    pub(super) tables: WgpuBuffer,
}

pub(super) struct WgpuFilterBrushBuffers {
    pub(super) blob: WgpuBuffer,
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
        self.tables.upload_cached(
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
            blob: WgpuBuffer::new(device, "tileink wgpu filter brush blob"),
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
        self.blob.upload_cached(
            device,
            queue,
            "tileink wgpu filter brush blob",
            &upload.blob,
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
        self.kernels.upload_cached(
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
        self.selectors.upload_cached(
            device,
            queue,
            "tileink wgpu filter turbulence selectors",
            &upload.selectors,
        );
        self.gradients.upload_cached(
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
        self.range_starts.upload_cached(
            device,
            queue,
            "tileink wgpu filter path range starts",
            &upload.range_starts,
        );
        self.range_ends.upload_cached(
            device,
            queue,
            "tileink wgpu filter path range ends",
            &upload.range_ends,
        );
        self.p0x
            .upload_cached(device, queue, "tileink wgpu filter path p0x", &upload.p0x);
        self.p0y
            .upload_cached(device, queue, "tileink wgpu filter path p0y", &upload.p0y);
        self.p1x
            .upload_cached(device, queue, "tileink wgpu filter path p1x", &upload.p1x);
        self.p1y
            .upload_cached(device, queue, "tileink wgpu filter path p1y", &upload.p1y);
    }
}
