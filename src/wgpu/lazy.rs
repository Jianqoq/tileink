use std::sync::OnceLock;

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
    label: &'static str,
    entry_point: &'static str,
}

impl LazyComputePipeline {
    pub(crate) const fn new(label: &'static str, entry_point: &'static str) -> Self {
        Self {
            pipeline: OnceLock::new(),
            label,
            entry_point,
        }
    }

    pub(crate) fn get(
        &self,
        device: &::wgpu::Device,
        layout: &::wgpu::PipelineLayout,
        module: &::wgpu::ShaderModule,
    ) -> &::wgpu::ComputePipeline {
        self.pipeline.get_or_init(|| {
            device.create_compute_pipeline(&::wgpu::ComputePipelineDescriptor {
                label: Some(self.label),
                layout: Some(layout),
                module,
                entry_point: Some(self.entry_point),
                compilation_options: ::wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        })
    }

    #[cfg(test)]
    pub(crate) fn is_initialized(&self) -> bool {
        self.pipeline.get().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::{LazyComputePipeline, LazyShaderModule};

    #[test]
    fn lazy_compute_pipeline_starts_uninitialized() {
        let pipeline = LazyComputePipeline::new("test pipeline", "main");

        assert!(!pipeline.is_initialized());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn lazy_gpu_resources_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<LazyShaderModule>();
        assert_send_sync::<LazyComputePipeline>();
    }
}
