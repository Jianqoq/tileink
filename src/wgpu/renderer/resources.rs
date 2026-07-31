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
            config: WgpuBuffer::new(device, "tileink wgpu canvas config"),
            scene_buffers: WgpuSceneBuffers::new(device, range_scatter_pipeline),
            scene_upload: WgpuSceneUploadStaging::default(),
            scan: WgpuScanBuffers::new(device),
            coarse: WgpuCoarseBuffers::new(device),
            fine_spills: WgpuBuffer::new(device, "tileink wgpu fine spills"),
            fine_indirect_args: WgpuBuffer::new(device, "tileink wgpu fine indirect args"),
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
        }
    }

    /// Installs this allocation set and returns the renderer's previously active set.
    pub(super) fn swap_with_renderer(mut self, renderer: &mut Renderer) -> Self {
        std::mem::swap(&mut self.config, &mut renderer.config);
        std::mem::swap(&mut self.scene_buffers, &mut renderer.scene_buffers);
        std::mem::swap(&mut self.scene_upload, &mut renderer.scene_upload);
        std::mem::swap(&mut self.scan, &mut renderer.scan);
        std::mem::swap(&mut self.coarse, &mut renderer.coarse);
        std::mem::swap(&mut self.fine_spills, &mut renderer.fine_spills);
        std::mem::swap(
            &mut self.fine_indirect_args,
            &mut renderer.fine_indirect_args,
        );
        std::mem::swap(&mut self.filter_transfers, &mut renderer.filter_transfers);
        std::mem::swap(&mut self.filter_brushes, &mut renderer.filter_brushes);
        std::mem::swap(&mut self.filter_convolves, &mut renderer.filter_convolves);
        std::mem::swap(&mut self.filter_turbulence, &mut renderer.filter_turbulence);
        std::mem::swap(&mut self.filter_paths, &mut renderer.filter_paths);
        std::mem::swap(&mut self.readback_target, &mut renderer.readback_target);
        std::mem::swap(
            &mut self.fine_portable_source,
            &mut renderer.fine_portable_source,
        );
        std::mem::swap(
            &mut self.fine_portable_target,
            &mut renderer.fine_portable_target,
        );
        std::mem::swap(
            &mut self.filter_target_snapshot,
            &mut renderer.filter_target_snapshot,
        );
        std::mem::swap(
            &mut self.root_target_texture,
            &mut renderer.root_target_texture,
        );
        std::mem::swap(&mut self.root_target_view, &mut renderer.root_target_view);
        std::mem::swap(&mut self.scratch, &mut renderer.scratch);
        std::mem::swap(&mut self.scratch_spares, &mut renderer.scratch_spares);
        std::mem::swap(&mut self.scratch_in_use, &mut renderer.scratch_in_use);
        self
    }

    fn for_target_size(mut self, size: (u32, u32)) -> Self {
        self.target_size = size;
        self
    }
}

impl Renderer {
    pub(super) fn acquire_local_scene_resources(&mut self, size: (u32, u32)) -> SceneResources {
        #[cfg(feature = "bench-internals")]
        if !self.reuse_local_scene_resources {
            return SceneResources::new(&self.device, self.range_scatter_pipeline.clone(), size);
        }
        let Some(fallback) = self.local_scene_resource_pool.pop() else {
            return SceneResources::new(&self.device, self.range_scatter_pipeline.clone(), size);
        };
        // One local scene and properly nested scenes naturally match LIFO order. Keep that path
        // O(1), and scan only when sibling traversal left another size at the back of the pool.
        if self.local_scene_resource_pool.is_empty() || fallback.target_size == size {
            return fallback.for_target_size(size);
        }
        let exact = self
            .local_scene_resource_pool
            .iter()
            .rposition(|resources| resources.target_size == size);
        let Some(index) = exact else {
            return fallback.for_target_size(size);
        };
        let resources = self.local_scene_resource_pool.swap_remove(index);
        self.local_scene_resource_pool.push(fallback);
        resources
    }

    pub(super) fn recycle_local_scene_resources(&mut self, resources: SceneResources) {
        #[cfg(feature = "bench-internals")]
        if !self.reuse_local_scene_resources {
            return;
        }
        // Commands already encoded for this frame still reference these buffers. Queue writes for
        // a sibling filter must not overwrite them before submission, so reuse starts next frame.
        self.pending_local_scene_resources.push(resources);
    }

    pub(super) fn begin_local_scene_resource_frame(&mut self) {
        self.local_scene_resource_pool
            .append(&mut self.pending_local_scene_resources);
    }

    /// Criterion-only switch that compares production pooling with the former allocation path.
    #[cfg(feature = "bench-internals")]
    #[doc(hidden)]
    pub fn set_local_scene_resource_reuse_for_benchmark(&mut self, reuse: bool) {
        self.reuse_local_scene_resources = reuse;
        if !reuse {
            self.local_scene_resource_pool.clear();
            self.pending_local_scene_resources.clear();
        }
    }

    /// Exercises one allocation/recycle cycle without encoding render commands.
    #[cfg(feature = "bench-internals")]
    #[doc(hidden)]
    pub fn cycle_local_scene_resources_for_benchmark(&mut self, size: (u32, u32)) {
        self.begin_local_scene_resource_frame();
        let resources = self.acquire_local_scene_resources(size);
        std::hint::black_box(resources.config.binding_key());
        self.recycle_local_scene_resources(resources);
    }

    /// Exercises differently sized sibling resources and their target preparation.
    #[cfg(feature = "bench-internals")]
    #[doc(hidden)]
    pub fn cycle_mixed_local_scene_resources_for_benchmark(&mut self, sizes: &[(u32, u32)]) {
        self.begin_local_scene_resource_frame();
        for &size in sizes {
            let mut resources = self.acquire_local_scene_resources(size);
            resources.prepare_minimum_targets_for_benchmark(&self.device, size);
            std::hint::black_box(resources.config.binding_key());
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
        self.readback_target.resize(device, size.0, size.1);
        self.fine_portable_source.resize(device, size.0, size.1);
        self.fine_portable_target.resize(device, size.0, size.1);
        self.filter_target_snapshot.resize(device, size.0, size.1);
        if self.scratch.is_empty() {
            self.scratch.push(WgpuTarget::new(device, size.0, size.1));
        } else {
            self.scratch[0].resize(device, size.0, size.1);
        }
    }
}
