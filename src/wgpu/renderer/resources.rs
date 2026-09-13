//! Reusable scene-bound GPU allocations for nested and cropped offscreen rendering.

use super::*;

impl SceneResources {
    pub(super) fn new(
        device: &::wgpu::Device,
        range_scatter_pipeline: Rc<WgpuRangeScatterPipeline>,
        size: (u32, u32),
    ) -> Self {
        Self {
            target_size: size,
            allocation: WgpuSceneAllocation {
                config: WgpuBuffer::new(device, "tileink wgpu canvas config"),
                scene_buffers: WgpuSceneBuffers::new(device, range_scatter_pipeline),
                scene_upload: WgpuSceneUploadStaging::default(),
                scan: WgpuScanBuffers::new(device),
                coarse: WgpuCoarseBuffers::new(device),
                fine_spills: WgpuBuffer::new(device, "tileink wgpu fine spills"),
                filter_transfers: WgpuFilterTransferBuffers::new(device),
                filter_brushes: WgpuFilterBrushBuffers::new(device),
                filter_convolves: WgpuFilterConvolveBuffers::new(device),
                filter_turbulence: WgpuFilterTurbulenceBuffers::new(device),
                filter_paths: WgpuFilterPathBuffers::new(device),
                readback_target: WgpuTarget::new(device, size.0, size.1),
                fine_portable_source: WgpuTarget::new(device, size.0, size.1),
                fine_portable_target: WgpuTarget::new(device, size.0, size.1),
                filter_target_snapshot: WgpuTarget::new(device, size.0, size.1),
                root_target_texture: None,
                root_target_view: None,
                scratch: Vec::new(),
                scratch_spares: Vec::new(),
                scratch_in_use: Vec::new(),
            },
        }
    }

    /// Installs this allocation set and returns the renderer's previously active set.
    pub(super) fn swap_with_renderer(mut self, renderer: &mut Renderer) -> Self {
        std::mem::swap(&mut self.allocation.config, &mut renderer.config);
        std::mem::swap(
            &mut self.allocation.scene_buffers,
            &mut renderer.scene_buffers,
        );
        std::mem::swap(
            &mut self.allocation.scene_upload,
            &mut renderer.scene_upload,
        );
        std::mem::swap(&mut self.allocation.scan, &mut renderer.scan);
        std::mem::swap(&mut self.allocation.coarse, &mut renderer.coarse);
        std::mem::swap(&mut self.allocation.fine_spills, &mut renderer.fine_spills);
        std::mem::swap(
            &mut self.allocation.filter_transfers,
            &mut renderer.filter_transfers,
        );
        std::mem::swap(
            &mut self.allocation.filter_brushes,
            &mut renderer.filter_brushes,
        );
        std::mem::swap(
            &mut self.allocation.filter_convolves,
            &mut renderer.filter_convolves,
        );
        std::mem::swap(
            &mut self.allocation.filter_turbulence,
            &mut renderer.filter_turbulence,
        );
        std::mem::swap(
            &mut self.allocation.filter_paths,
            &mut renderer.filter_paths,
        );
        std::mem::swap(
            &mut self.allocation.readback_target,
            &mut renderer.readback_target,
        );
        std::mem::swap(
            &mut self.allocation.fine_portable_source,
            &mut renderer.fine_portable_source,
        );
        std::mem::swap(
            &mut self.allocation.fine_portable_target,
            &mut renderer.fine_portable_target,
        );
        std::mem::swap(
            &mut self.allocation.filter_target_snapshot,
            &mut renderer.filter_target_snapshot,
        );
        std::mem::swap(
            &mut self.allocation.root_target_texture,
            &mut renderer.root_target_texture,
        );
        std::mem::swap(
            &mut self.allocation.root_target_view,
            &mut renderer.root_target_view,
        );
        std::mem::swap(&mut self.allocation.scratch, &mut renderer.scratch);
        std::mem::swap(
            &mut self.allocation.scratch_spares,
            &mut renderer.scratch_spares,
        );
        std::mem::swap(
            &mut self.allocation.scratch_in_use,
            &mut renderer.scratch_in_use,
        );
        self
    }
}

impl Renderer {
    pub(super) fn acquire_local_scene_resources(&mut self, size: (u32, u32)) -> SceneResources {
        #[cfg(feature = "bench-internals")]
        if !self.reuse_local_scene_resources {
            return SceneResources::new(&self.device, self.range_scatter_pipeline.clone(), size);
        }
        self.local_scene_resources.acquire(size).unwrap_or_else(|| {
            SceneResources::new(&self.device, self.range_scatter_pipeline.clone(), size)
        })
    }

    pub(super) fn recycle_local_scene_resources(&mut self, resources: SceneResources) {
        #[cfg(feature = "bench-internals")]
        if !self.reuse_local_scene_resources {
            return;
        }
        // Commands already encoded for this frame still reference these buffers. Queue writes for
        // a sibling filter must not overwrite them before submission, so reuse starts next frame.
        self.local_scene_resources.recycle(resources);
    }

    /// Criterion-only switch that compares production pooling with the former allocation path.
    #[cfg(feature = "bench-internals")]
    #[doc(hidden)]
    pub fn set_local_scene_resource_reuse_for_benchmark(&mut self, reuse: bool) {
        self.reuse_local_scene_resources = reuse;
        if !reuse {
            self.local_scene_resources.clear();
        }
    }

    /// Exercises one allocation/recycle cycle without encoding render commands.
    #[cfg(feature = "bench-internals")]
    #[doc(hidden)]
    pub fn cycle_local_scene_resources_for_benchmark(&mut self, size: (u32, u32)) {
        self.local_scene_resources.begin_frame();
        let resources = self.acquire_local_scene_resources(size);
        std::hint::black_box(resources.allocation.config.binding_key());
        self.recycle_local_scene_resources(resources);
    }

    /// Exercises differently sized sibling resources and their target preparation.
    #[cfg(feature = "bench-internals")]
    #[doc(hidden)]
    pub fn cycle_mixed_local_scene_resources_for_benchmark(&mut self, sizes: &[(u32, u32)]) {
        self.local_scene_resources.begin_frame();
        for &size in sizes {
            let mut resources = self.acquire_local_scene_resources(size);
            resources.prepare_minimum_targets_for_benchmark(&self.device, size);
            std::hint::black_box(resources.allocation.config.binding_key());
            self.recycle_local_scene_resources(resources);
        }
    }

    /// Exercises the production fixed-target resize path without scene construction noise.
    #[cfg(feature = "bench-internals")]
    #[doc(hidden)]
    pub fn resize_internal_targets_for_benchmark(&mut self, sizes: &[(u32, u32)]) {
        for &size in sizes {
            self.readback_target.resize(&self.device, size.0, size.1);
            self.fine_portable_source
                .resize(&self.device, size.0, size.1);
            self.fine_portable_target
                .resize(&self.device, size.0, size.1);
            self.filter_target_snapshot
                .resize(&self.device, size.0, size.1);
            if self.scratch.is_empty() {
                self.scratch
                    .push(WgpuTarget::new(&self.device, size.0, size.1));
            } else {
                self.scratch[0].resize(&self.device, size.0, size.1);
            }
            std::hint::black_box(self.readback_target.byte_len());
        }
    }
}

#[cfg(feature = "bench-internals")]
impl SceneResources {
    /// Mirrors the fixed targets plus the minimum scratch target prepared by a local filter.
    fn prepare_minimum_targets_for_benchmark(&mut self, device: &::wgpu::Device, size: (u32, u32)) {
        self.allocation
            .readback_target
            .resize(device, size.0, size.1);
        self.allocation
            .fine_portable_source
            .resize(device, size.0, size.1);
        self.allocation
            .fine_portable_target
            .resize(device, size.0, size.1);
        self.allocation
            .filter_target_snapshot
            .resize(device, size.0, size.1);
        if self.allocation.scratch.is_empty() {
            self.allocation
                .scratch
                .push(WgpuTarget::new(device, size.0, size.1));
        } else {
            self.allocation.scratch[0].resize(device, size.0, size.1);
        }
    }
}
