use super::benchmark_gpu;

use peniko::Color;
use tileink::{WgpuRenderer, WgpuRendererOptions};

/// A device and optional driver cache shared across independent benchmark renderers.
/// Scene, upload and history state are recreated for every measurement batch.
pub struct BenchContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    options: WgpuRendererOptions,
}

impl BenchContext {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let pipeline_cache = device
            .features()
            .contains(wgpu::Features::PIPELINE_CACHE)
            .then(|| {
                // No external cache bytes are imported; the driver owns this process-local cache.
                unsafe {
                    device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                        label: Some("retained benchmark pipelines"),
                        data: None,
                        fallback: true,
                    })
                }
            });
        let context = Self {
            device: device.clone(),
            queue: queue.clone(),
            options: WgpuRendererOptions { pipeline_cache },
        };
        let metadata = serde_json::json!({
            "shared_pipeline_cache_present": context.shared_pipeline_cache_present(),
        });
        eprintln!("benchmark context: {metadata}");
        context
    }

    #[allow(dead_code)]
    pub fn default_device() -> Self {
        let (device, queue) = benchmark_gpu::default_device(None);
        Self::new(&device, &queue)
    }

    pub fn shared_pipeline_cache_present(&self) -> bool {
        self.options.pipeline_cache.is_some()
    }

    pub fn renderer(&self) -> WgpuRenderer {
        // Sharing only the pipeline cache fixes repeated untimed driver compilation without
        // carrying scene/history state or resource capacities across Criterion batches.
        WgpuRenderer::new_with_options(
            &self.device,
            &self.queue,
            super::WIDTH,
            super::HEIGHT,
            Color::TRANSPARENT,
            self.options.clone(),
        )
    }
}
