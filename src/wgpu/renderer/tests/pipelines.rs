use super::*;

#[test]
fn wgpu_renderer_lazily_initializes_and_reuses_compute_pipelines() {
    if !run_wgpu_tests() {
        return;
    }

    let mut path = BezPath::new();
    path.move_to((2.0, 2.0));
    path.line_to((30.0, 4.0));
    path.line_to((12.0, 30.0));
    path.close_path();
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_path(
        path,
        Color::from_rgb8(40, 120, 220),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = new_test_renderer(32, 32, Color::TRANSPARENT);
    if renderer.scan_pipeline.is_none()
        || renderer.cumsum.is_none()
        || renderer.coarse_pipeline.is_none()
        || renderer.fine.is_none()
        || renderer.filter.is_none()
    {
        return;
    }
    assert_eq!(initialized_compute_pipeline_counts(&renderer), [0; 4]);
    assert_eq!(renderer.pipeline_compilation_epoch(), 0);

    renderer.render(&canvas);
    let first_render = initialized_compute_pipeline_counts(&renderer);
    let first_epoch = renderer.pipeline_compilation_epoch();
    assert!(
        first_render[..4].iter().all(|count| *count > 0),
        "tile stages should initialize only the pipelines used by the first render: {first_render:?}"
    );
    assert!(first_epoch > 0);
    renderer.render(&canvas);
    assert_eq!(initialized_compute_pipeline_counts(&renderer), first_render);
    assert_eq!(renderer.pipeline_compilation_epoch(), first_epoch);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn wgpu_renderer_populates_and_reloads_pipeline_cache_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let instance = ::wgpu::Instance::new(::wgpu::InstanceDescriptor {
        backends: ::wgpu::Backends::VULKAN,
        flags: ::wgpu::InstanceFlags::empty(),
        memory_budget_thresholds: ::wgpu::MemoryBudgetThresholds::default(),
        backend_options: ::wgpu::BackendOptions::default(),
        display: None,
    });
    let Ok(adapter) =
        pollster::block_on(instance.request_adapter(&::wgpu::RequestAdapterOptions {
            power_preference: ::wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
    else {
        return;
    };
    if !adapter
        .features()
        .contains(::wgpu::Features::PIPELINE_CACHE)
    {
        return;
    }
    let optional = ::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
        | ::wgpu::Features::TEXTURE_BINDING_ARRAY
        | ::wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
    let required_features = ::wgpu::Features::PIPELINE_CACHE | (adapter.features() & optional);
    let Ok((device, queue)) =
        pollster::block_on(adapter.request_device(&::wgpu::DeviceDescriptor {
            label: Some("tileink pipeline cache test device"),
            required_features,
            required_limits: adapter.limits(),
            memory_hints: ::wgpu::MemoryHints::Performance,
            trace: ::wgpu::Trace::Off,
            experimental_features: ::wgpu::ExperimentalFeatures::disabled(),
        }))
    else {
        return;
    };
    // SAFETY: no initial cache data is supplied.
    let cache = unsafe {
        device.create_pipeline_cache(&::wgpu::PipelineCacheDescriptor {
            label: Some("tileink pipeline cache test"),
            data: None,
            fallback: true,
        })
    };

    let mut canvas = Canvas::new(64, 48, 1.0);
    canvas.push_filter_layer(
        Filter::Blur {
            std_dev_x: 2.0,
            std_dev_y: 2.0,
            sampling: BlurSampling::FULL_RES,
        },
        Region::rect(Rect::new(4.0, 4.0, 60.0, 44.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(12.0, 10.0, 52.0, 38.0),
        crate::Radius::all(5.0),
        Color::from_rgb8(40, 120, 220),
    );
    canvas.pop_layer();

    let mut renderer = Renderer::new_with_options(
        &device,
        &queue,
        64,
        48,
        Color::TRANSPARENT,
        RendererOptions {
            pipeline_cache: Some(cache.clone()),
        },
    );
    renderer.render(&canvas);
    let data = cache
        .get_data()
        .filter(|data| !data.is_empty())
        .expect("rendered compute pipelines should populate the Vulkan cache");

    // SAFETY: `data` came directly from a wgpu pipeline cache for this same device.
    let reloaded = unsafe {
        device.create_pipeline_cache(&::wgpu::PipelineCacheDescriptor {
            label: Some("tileink reloaded pipeline cache test"),
            data: Some(&data),
            fallback: false,
        })
    };
    let mut renderer = Renderer::new_with_options(
        &device,
        &queue,
        64,
        48,
        Color::TRANSPARENT,
        RendererOptions {
            pipeline_cache: Some(reloaded.clone()),
        },
    );
    renderer.render(&canvas);
    assert!(reloaded.get_data().is_some());
}
