pub(super) use peniko::{
    Color, Compose, Extend, Gradient, Mix,
    kurbo::{Affine, BezPath, Line, Rect, Shape},
};

pub(super) use super::super::{Renderer, RendererOptions, WgpuRenderTargetId};
pub(super) use crate::wgpu::coarse::force_coarse_emit_chunks_for_test;
pub(super) use crate::wgpu::commands::WgpuCommandBatch;
pub(super) use crate::wgpu::{
    buffer::{WgpuBuffer, WgpuRangeScatter, WgpuRangeScatterPipeline},
    lazy::PipelineCompilationTracker,
};
pub(super) use crate::{
    Canvas, FillRule, Image, ImageKey, PatternSampling, RetainedLayerDescriptor, RetainedNodeId,
    RetainedParent, RetainedScene, TextContext, TextFontSystem, TextLayoutOptions,
    debug::{RenderDebugOptions, RenderOptions},
    shared::{
        bounds::Bounds,
        brush::Brush,
        execution::ExecOp,
        gpu_coarse::{FineTileKind, PtclRecord},
        layer::{
            filter::{
                BlurSampling, COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE,
                ColorChannel, CompositeOperator, ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting,
                DisplacementMap, Filter, FilterInput, FilterPrimitive, FilterPrimitiveKind,
                LightSource, MorphologyOperator, RectLiquidGlass, SpecularLighting, Turbulence,
                TurbulenceKind,
            },
            mask::{Mask, MaskKind},
            region::Region,
        },
        tile_seg_range::TileSegmentRange,
    },
};
pub(super) fn run_wgpu_tests() -> bool {
    std::env::var("TILEINK_RUN_WGPU_TESTS").as_deref() == Ok("1")
        || std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() == Ok("1")
}

pub(super) fn new_test_renderer(width: u32, height: u32, clear: Color) -> Renderer {
    let portable = std::env::var("TILEINK_WGPU_MODE").as_deref() == Ok("portable");
    let Some((device, queue)) = shared_wgpu_test_device(portable) else {
        return Renderer::new_default_device(width, height, clear);
    };
    Renderer::new(device, queue, width, height, clear)
}

pub(super) fn shared_wgpu_test_device(
    portable: bool,
) -> Option<&'static (::wgpu::Device, ::wgpu::Queue)> {
    use std::sync::OnceLock;

    static NATIVE: OnceLock<Option<(::wgpu::Device, ::wgpu::Queue)>> = OnceLock::new();
    static PORTABLE: OnceLock<Option<(::wgpu::Device, ::wgpu::Queue)>> = OnceLock::new();
    let slot = if portable { &PORTABLE } else { &NATIVE };
    slot.get_or_init(|| {
        let instance =
            ::wgpu::Instance::new(::wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&::wgpu::RequestAdapterOptions {
                power_preference: ::wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))
            .ok()?;
        // Match the production native device: omitting adapter-specific formats silently
        // selects portable fine and leaves native rendering regressions untested.
        let optional_native = ::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | ::wgpu::Features::TIMESTAMP_QUERY
            | ::wgpu::Features::TEXTURE_BINDING_ARRAY
            | ::wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
        pollster::block_on(adapter.request_device(&::wgpu::DeviceDescriptor {
            label: Some(if portable {
                "tileink portable shared test device"
            } else {
                "tileink native shared test device"
            }),
            required_features: if portable {
                ::wgpu::Features::empty()
            } else {
                adapter.features() & optional_native
            },
            required_limits: adapter.limits(),
            memory_hints: ::wgpu::MemoryHints::Performance,
            trace: ::wgpu::Trace::Off,
            experimental_features: ::wgpu::ExperimentalFeatures::disabled(),
        }))
        .ok()
    })
    .as_ref()
}

pub(super) fn test_turbulence(kind: TurbulenceKind, seed: i32, num_octaves: u32) -> Turbulence {
    Turbulence {
        base_frequency_x: 0.07,
        base_frequency_y: 0.11,
        num_octaves,
        seed,
        stitch_tiles: false,
        kind,
        linear_rgb: false,
        transform_x: 0.0,
        transform_y: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        tile_x: 0.0,
        tile_y: 0.0,
        tile_width: 32.0,
        tile_height: 24.0,
    }
}

pub(super) fn render_native_wgpu(canvas: &Canvas) -> crate::shared::image::Image {
    let mut renderer = Renderer::new_default_device(
        canvas.physical_width(),
        canvas.physical_height(),
        Color::TRANSPARENT,
    );
    renderer.prepare_scene(canvas);
    assert!(
        renderer.render_prepared_tile_plan(canvas),
        "expected canvas to render through native wgpu path"
    );
    renderer.image()
}

pub(super) fn assert_images_near(
    actual: &crate::shared::image::Image,
    expected: &crate::shared::image::Image,
    tolerance: u8,
    context: &str,
) {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    for y in 0..actual.height {
        for x in 0..actual.width {
            let a = actual.rgba8_at(x, y);
            let e = expected.rgba8_at(x, y);
            for channel in 0..4 {
                let diff = a[channel].abs_diff(e[channel]);
                assert!(
                    diff <= tolerance,
                    "{context} mismatch at ({x}, {y}) channel {channel}: actual {a:?}, expected {e:?}"
                );
            }
        }
    }
}

pub(super) fn assert_profile_has(profile: &crate::WgpuRenderProfile, name: &'static str) {
    assert!(
        profile.entries().iter().any(|entry| entry.name == name),
        "profile missing {name}; entries: {:?}",
        profile.entries(),
    );
}

pub(super) fn assert_profile_missing(profile: &crate::WgpuRenderProfile, name: &'static str) {
    assert!(
        !profile.entries().iter().any(|entry| entry.name == name),
        "profile unexpectedly included {name}; entries: {:?}",
        profile.entries()
    );
}

pub(super) fn initialized_compute_pipeline_counts(renderer: &Renderer) -> [usize; 4] {
    [
        renderer
            .scan_pipeline
            .as_ref()
            .map_or(0, |pipeline| pipeline.initialized_pipeline_count()),
        renderer
            .cumsum
            .as_ref()
            .map_or(0, |pipeline| pipeline.initialized_pipeline_count()),
        renderer
            .coarse_pipeline
            .as_ref()
            .map_or(0, |pipeline| pipeline.initialized_pipeline_count()),
        renderer
            .fine
            .as_ref()
            .map_or(0, |pipeline| pipeline.initialized_pipeline_count()),
    ]
}

pub(super) fn read_render_target_u32(
    renderer: &Renderer,
    target: WgpuRenderTargetId,
    len: usize,
) -> Vec<u32> {
    let texture = match target {
        WgpuRenderTargetId::Main => renderer.readback_target.texture(),
        WgpuRenderTargetId::Scratch(ix) => renderer.scratch[ix].texture(),
    };
    let bytes = read_texture_rgba8(
        renderer.device(),
        renderer.queue(),
        texture,
        renderer.size.0,
        renderer.size.1,
    );
    bytemuck::cast_slice(&bytes)[..len].to_vec()
}

/// Runs the first compiled draw batch with its fused layer stack.
///
/// Clip commands are structural execution-plan entries, not ordinary draws in
/// the child batch. Coarse tests must follow the compiled contract used by the
/// renderer or they silently omit the clip wrappers they intend to inspect.
pub(super) fn coarse_first_draw_batch(renderer: &mut Renderer, canvas: &Canvas) {
    let (batch_id, layer_stack) = renderer
        .plan
        .as_ref()
        .expect("prepared execution plan")
        .ops
        .iter()
        .find_map(|op| match op {
            ExecOp::DrawBatch {
                batch_id,
                layer_stack,
                ..
            } => Some((*batch_id, layer_stack.clone())),
            _ => None,
        })
        .expect("execution plan contains a draw batch");
    // Path clips in the fused stack consume scan/cumsum backdrops before
    // coarse can decide whether the clip is a no-op or needs wrapper particles.
    renderer.scan_for_test();
    renderer.cumsum_for_test();
    renderer.coarse_batch(
        canvas,
        batch_id,
        batch_id + 1,
        layer_stack.start as u32,
        layer_stack.end as u32,
    );
}

pub(super) fn read_ptcl_tags(renderer: &Renderer, len: usize) -> Vec<u32> {
    read_ptcl_records(renderer, len)
        .into_iter()
        .map(|record| record.tag)
        .collect()
}

pub(super) fn read_ptcl_colors(renderer: &Renderer, len: usize) -> Vec<u32> {
    read_ptcl_records(renderer, len)
        .into_iter()
        .map(|record| record.color)
        .collect()
}

pub(super) fn read_ptcl_records(renderer: &Renderer, len: usize) -> Vec<PtclRecord> {
    renderer.coarse.read_ptcl_records(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.tile_count,
        len,
    )
}

pub(super) fn read_texture_rgba8(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    texture: &::wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let row_bytes = width as ::wgpu::BufferAddress * 4;
    let padded_row_bytes = row_bytes.next_multiple_of(::wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64);
    let readback = device.create_buffer(&::wgpu::BufferDescriptor {
        label: Some("tileink wgpu renderer texture readback"),
        size: padded_row_bytes * height as u64,
        usage: ::wgpu::BufferUsages::COPY_DST | ::wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&::wgpu::CommandEncoderDescriptor {
        label: Some("tileink wgpu renderer texture readback copy"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        ::wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: ::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: (height > 1).then_some(padded_row_bytes as u32),
                rows_per_image: None,
            },
        },
        ::wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(::wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap()
        });
    device.poll(::wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();

    let view = readback
        .slice(..)
        .get_mapped_range()
        .expect("read mapped wgpu test readback buffer");
    let mut tight = Vec::with_capacity((row_bytes * height as u64) as usize);
    for row in 0..height as usize {
        let start = row * padded_row_bytes as usize;
        tight.extend_from_slice(&view[start..start + row_bytes as usize]);
    }
    drop(view);
    readback.unmap();
    tight
}

pub(super) fn external_target(
    device: &::wgpu::Device,
    size: (u32, u32),
    label: &'static str,
) -> ::wgpu::Texture {
    device.create_texture(&::wgpu::TextureDescriptor {
        label: Some(label),
        size: ::wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: ::wgpu::TextureDimension::D2,
        format: ::wgpu::TextureFormat::Rgba8Unorm,
        usage: ::wgpu::TextureUsages::COPY_SRC
            | ::wgpu::TextureUsages::COPY_DST
            | ::wgpu::TextureUsages::STORAGE_BINDING
            | ::wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}
