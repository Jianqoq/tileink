#![cfg(feature = "wgpu")]

#[allow(dead_code)]
#[path = "../src/shared/gpu_constants.rs"]
mod gpu_constants;

#[path = "../src/wgpu/shader_variants.rs"]
mod shader_variants;

#[test]
fn generated_workgroups_match_host_dispatch_constants() {
    // Validate compiled entry metadata, not source spelling: a stale shader literal must fail.
    for (source, width) in [
        (
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_fine.wgsl")),
            gpu_constants::FINE_WORKGROUP_SIZE,
        ),
        (
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_fine_web.wgsl")),
            gpu_constants::FINE_WORKGROUP_SIZE,
        ),
        (
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_filter.wgsl")),
            gpu_constants::FILTER_WORKGROUP_SIZE,
        ),
        (
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_filter_web.wgsl")),
            gpu_constants::FILTER_WORKGROUP_SIZE,
        ),
    ] {
        for textures_enabled in [false, true] {
            let source =
                shader_variants::patch_image_resource_shader_source(source, textures_enabled);
            let module = wgpu::naga::front::wgsl::parse_str(&source).unwrap();
            assert!(!module.entry_points.is_empty());
            for entry in module.entry_points {
                let expected = if entry.name == "filter_blur_shared_region" {
                    [
                        gpu_constants::SHARED_BLUR_TILE_WIDTH,
                        gpu_constants::SHARED_BLUR_TILE_HEIGHT,
                        1,
                    ]
                } else {
                    [width, 1, 1]
                };
                assert_eq!(entry.workgroup_size, expected, "{}", entry.name);
            }
        }
    }
    assert_eq!(
        gpu_constants::FINE_WORKGROUP_SIZE,
        gpu_constants::TILE_SIZE.pow(2)
    );
}

#[test]
#[ignore = "requires a Vulkan GPU"]
fn filter_dispatch_rows_follow_specialized_workgroup_width() {
    use wgpu::util::DeviceExt;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    // Execute the production index helper with other lane widths. This catches a literal
    // row stride even when today's default happens to have the same numerical value.
    let header = include_str!("../src/wgpu/shaders/filter/header.wgsl");
    let start = header.find("fn filter_region_index(").unwrap();
    let end = header[start..].find("\n}").unwrap() + start + 2;
    let function = &header[start..end];
    for width in [32, 64, gpu_constants::FILTER_WORKGROUP_SIZE] {
        let count = width * 2 * 3;
        let source = format!(
            r#"
const FILTER_WORKGROUP_SIZE: u32 = {width}u;
struct Config {{ dispatch_width: u32 }}
const config = Config(2u);
{function}
@group(0) @binding(0) var<storage, read_write> result: array<u32>;
@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn probe(@builtin(global_invocation_id) gid: vec3<u32>) {{
    result[gid.y * {width}u * 2u + gid.x] = filter_region_index(gid);
}}
"#
        );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("filter dispatch row regression"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("probe"),
            compilation_options: Default::default(),
            cache: None,
        });
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&vec![u32::MAX; count as usize]),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(count) * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(2, 3, 1);
        }
        encoder.copy_buffer_to_buffer(&buffer, 0, &readback, 0, u64::from(count) * 4);
        queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap();
            });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receive.recv().unwrap().unwrap();
        let bytes = readback.slice(..).get_mapped_range().unwrap();
        let actual: &[u32] = bytemuck::cast_slice(&bytes);
        assert_eq!(actual, (0..count).collect::<Vec<_>>(), "width {width}");
        drop(bytes);
        readback.unmap();
    }
}
