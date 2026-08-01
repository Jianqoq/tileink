use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use super::dxil::PrecompiledDxil;

#[derive(Clone, Default)]
pub(crate) struct PipelineCompilationTracker {
    epoch: Arc<AtomicU64>,
    precompiled_dxil: Arc<AtomicU64>,
}

impl PipelineCompilationTracker {
    pub(crate) fn record(&self) {
        self.record_source(PipelineSource::RuntimeWgsl);
    }

    fn record_source(&self, source: PipelineSource) {
        self.epoch.fetch_add(1, Ordering::Relaxed);
        if source == PipelineSource::PrecompiledDxil {
            self.precompiled_dxil.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }

    pub(crate) fn precompiled_dxil_count(&self) -> u64 {
        self.precompiled_dxil.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PipelineSource {
    RuntimeWgsl,
    PrecompiledDxil,
}

/// Defers shader-module creation until a pipeline using that shader is dispatched.
///
/// Pipeline layouts still exist eagerly because bind groups need them during command encoding,
/// while shader validation and backend compilation are kept off the renderer construction path.
pub(crate) struct LazyShaderModule {
    module: OnceLock<::wgpu::ShaderModule>,
    label: &'static str,
}

impl LazyShaderModule {
    pub(crate) const fn new(label: &'static str) -> Self {
        Self {
            module: OnceLock::new(),
            label,
        }
    }

    pub(crate) fn get(
        &self,
        device: &::wgpu::Device,
        source: impl FnOnce() -> ::wgpu::ShaderSource<'static>,
    ) -> &::wgpu::ShaderModule {
        self.module.get_or_init(|| {
            device.create_shader_module(::wgpu::ShaderModuleDescriptor {
                label: Some(self.label),
                source: source(),
            })
        })
    }
}

/// Compiles one compute pipeline on first use and reuses it for the renderer's lifetime.
///
/// `OnceLock` preserves the renderer's thread-safe ownership semantics and guarantees that
/// concurrent first users cannot compile the same pipeline more than once.
pub(crate) struct LazyComputePipeline {
    pipeline: OnceLock<::wgpu::ComputePipeline>,
    pipeline_cache: Option<::wgpu::PipelineCache>,
    compilation_tracker: PipelineCompilationTracker,
    label: &'static str,
    entry_point: &'static str,
    precompiled_dxil: Option<PrecompiledDxil>,
}

impl LazyComputePipeline {
    pub(crate) fn new(
        label: &'static str,
        entry_point: &'static str,
        pipeline_cache: Option<&::wgpu::PipelineCache>,
        compilation_tracker: &PipelineCompilationTracker,
    ) -> Self {
        Self {
            pipeline: OnceLock::new(),
            pipeline_cache: pipeline_cache.cloned(),
            compilation_tracker: compilation_tracker.clone(),
            label,
            entry_point,
            precompiled_dxil: None,
        }
    }

    pub(crate) fn new_with_dxil(
        label: &'static str,
        entry_point: &'static str,
        pipeline_cache: Option<&::wgpu::PipelineCache>,
        compilation_tracker: &PipelineCompilationTracker,
        precompiled_dxil: Option<PrecompiledDxil>,
    ) -> Self {
        Self {
            precompiled_dxil,
            ..Self::new(label, entry_point, pipeline_cache, compilation_tracker)
        }
    }

    pub(crate) fn get(
        &self,
        device: &::wgpu::Device,
        layout: &::wgpu::PipelineLayout,
        module: &::wgpu::ShaderModule,
    ) -> &::wgpu::ComputePipeline {
        self.pipeline.get_or_init(|| {
            self.create_pipeline(device, layout, module, PipelineSource::RuntimeWgsl)
        })
    }

    pub(crate) fn get_with_fallback<'a>(
        &self,
        device: &::wgpu::Device,
        layout: &::wgpu::PipelineLayout,
        fallback_module: impl FnOnce() -> &'a ::wgpu::ShaderModule,
    ) -> &::wgpu::ComputePipeline {
        self.pipeline.get_or_init(|| {
            if let Some(dxil) = self.precompiled_dxil {
                let module = dxil.create_shader_module(device, self.label);
                self.create_pipeline(device, layout, &module, PipelineSource::PrecompiledDxil)
            } else {
                self.create_pipeline(
                    device,
                    layout,
                    fallback_module(),
                    PipelineSource::RuntimeWgsl,
                )
            }
        })
    }

    fn create_pipeline(
        &self,
        device: &::wgpu::Device,
        layout: &::wgpu::PipelineLayout,
        module: &::wgpu::ShaderModule,
        source: PipelineSource,
    ) -> ::wgpu::ComputePipeline {
        let pipeline = device.create_compute_pipeline(&::wgpu::ComputePipelineDescriptor {
            label: Some(self.label),
            layout: Some(layout),
            module,
            entry_point: Some(self.entry_point),
            compilation_options: ::wgpu::PipelineCompilationOptions::default(),
            cache: self.pipeline_cache.as_ref(),
        });
        self.compilation_tracker.record_source(source);
        pipeline
    }

    #[cfg(test)]
    pub(crate) fn is_initialized(&self) -> bool {
        self.pipeline.get().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::{LazyComputePipeline, LazyShaderModule, PipelineCompilationTracker};

    #[test]
    fn lazy_compute_pipeline_starts_uninitialized() {
        let tracker = PipelineCompilationTracker::default();
        let pipeline = LazyComputePipeline::new("test pipeline", "main", None, &tracker);

        assert!(!pipeline.is_initialized());
        assert_eq!(tracker.precompiled_dxil_count(), 0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn lazy_gpu_resources_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<LazyShaderModule>();
        assert_send_sync::<LazyComputePipeline>();
        assert_send_sync::<PipelineCompilationTracker>();
    }
}
