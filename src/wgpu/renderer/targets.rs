//! Render-target bindings and active damage helpers.

use super::*;

impl Renderer {
    pub(super) fn acquire_scratch(&mut self) -> Option<RenderTargetId> {
        self.scratch_slots.acquire().map(RenderTargetId::Scratch)
    }

    pub(super) fn release_scratch(&mut self, target: RenderTargetId) {
        let RenderTargetId::Scratch(ix) = target else {
            return;
        };
        self.scratch_slots.release(ix);
    }

    pub(super) fn take_scratch_target(&mut self, target: RenderTargetId) -> Option<WgpuTarget> {
        let RenderTargetId::Scratch(ix) = target else {
            return None;
        };
        // A retained surface temporarily owns the texture that occupied this slot. Reuse the
        // displaced slot target on the inverse transfer instead of allocating and immediately
        // dropping a full-size placeholder texture every incremental frame.
        let replacement = self
            .scratch_spares
            .pop()
            .unwrap_or_else(|| WgpuTarget::new(&self.device, self.size.0, self.size.1));
        self.scratch_slots.release(ix);
        Some(std::mem::replace(&mut self.scratch[ix], replacement))
    }

    pub(super) fn install_scratch_target(&mut self, index: usize, target: WgpuTarget) {
        debug_assert!(target.fits(self.size));
        let displaced = std::mem::replace(&mut self.scratch[index], target);
        self.scratch_spares.push(displaced);
        self.scratch_slots.occupy(index);
    }

    pub(super) fn install_scratch_render_target(
        &mut self,
        target_id: RenderTargetId,
        target: WgpuTarget,
    ) {
        let RenderTargetId::Scratch(index) = target_id else {
            unreachable!("retained surfaces can only occupy scratch targets")
        };
        self.install_scratch_target(index, target);
    }

    pub(super) fn render_target_view(&self, target: RenderTargetId) -> &::wgpu::TextureView {
        match target {
            RenderTargetId::Main => self
                .root_target_view
                .as_ref()
                .unwrap_or(self.readback_target.view()),
            RenderTargetId::Scratch(ix) => self.scratch[ix].view(),
        }
    }

    pub(super) fn render_target_texture(&self, target: RenderTargetId) -> Option<&::wgpu::Texture> {
        match target {
            RenderTargetId::Main => Some(
                self.root_target_texture
                    .as_ref()
                    .unwrap_or(self.readback_target.texture()),
            ),
            RenderTargetId::Scratch(ix) => Some(self.scratch.get(ix)?.texture()),
        }
    }

    pub(super) fn snapshot_filter_target(
        &self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        bounds: Bounds,
    ) -> Option<&::wgpu::TextureView> {
        copy_texture_region(
            commands.encoder(),
            self.render_target_texture(target)?,
            self.filter_target_snapshot.texture(),
            self.size,
            bounds,
        );
        Some(self.filter_target_snapshot.view())
    }

    pub(super) fn filter_brush_bindings(&self) -> WgpuFilterBrushBindings<'_> {
        let image_resources = self.scene_buffers.image_resource_bindings();
        WgpuFilterBrushBindings {
            blob: self.filter_brushes.blob.buffer(),
            image_resource_atlas: image_resources.atlas,
            image_resource_sampler: image_resources.sampler,
            image_resource_texture_views: image_resources.texture_views,
            image_resource_dummy_texture: image_resources.dummy_texture,
        }
    }

    pub(super) fn filter_turbulence_bindings(&self) -> WgpuFilterTurbulenceBindings<'_> {
        WgpuFilterTurbulenceBindings {
            selectors: self.filter_turbulence.selectors.buffer(),
            gradients: self.filter_turbulence.gradients.buffer(),
        }
    }

    pub(super) fn filter_path_bindings(&self) -> WgpuFilterPathBindings<'_> {
        WgpuFilterPathBindings {
            range_starts: self.filter_paths.range_starts.buffer(),
            range_ends: self.filter_paths.range_ends.buffer(),
            p0x: self.filter_paths.p0x.buffer(),
            p0y: self.filter_paths.p0y.buffer(),
            p1x: self.filter_paths.p1x.buffer(),
            p1y: self.filter_paths.p1y.buffer(),
        }
    }
}
