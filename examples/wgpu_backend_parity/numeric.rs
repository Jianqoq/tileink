use super::{Result, gpu};
use wgpu::util::DeviceExt;

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn pattern_transform_preserves_texel_boundaries() -> Result<()> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
        backend_options: wgpu::BackendOptions {
            dx12: wgpu::Dx12BackendOptions {
                shader_compiler: wgpu::Dx12Compiler::DynamicDxc {
                    dxc_path: std::env::var("TILEINK_PARITY_DXCOMPILER")?,
                },
                ..Default::default()
            },
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let coefficient = f32::from_bits(1024512194);
    let mut input: Vec<[f32; 4]> = [-8192.5, -74.5, 0.5, 74.5, 150.5, 300.5, 8192.5]
        .into_iter()
        .flat_map(|x| {
            [
                [-coefficient, coefficient, x, x],
                [coefficient, -coefficient, x, x],
            ]
        })
        .collect();
    let mut expected = vec![0.0_f32; input.len()];
    input.extend([
        [1.0, 0.0, 12.5, 17.25],
        [0.0, 1.0, 12.5, 17.25],
        [2.0, 0.5, 8.0, 4.0],
        [-2.0, 0.5, 8.0, 4.0],
    ]);
    expected.extend([12.5, 17.25, 18.0, -14.0]);
    let expected: Vec<u32> = expected
        .into_iter()
        .flat_map(|value| [value.to_bits(), (value + 3.25).to_bits()])
        .collect();
    let source = format!(
        "{}\n{}",
        include_str!("../../src/wgpu/shaders/shared/pattern_transform.wgsl"),
        "@group(0) @binding(0) var<storage, read> input: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = input[id.x];
    output[2u * id.x] = bitcast<u32>(pattern_transform_component(p.x, p.y, 0.0, p.z, p.w));
    output[2u * id.x + 1u] = bitcast<u32>(pattern_transform_component(p.x, p.y, 3.25, p.z, p.w));
}"
    );
    let mut luid = None;
    for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
        let route = gpu::create(&instance, backend, false, luid.as_deref())?;
        luid = Some(route.identity.clone());
        let device = route.renderer.device();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pattern coordinate semantics"),
            source: wgpu::ShaderSource::Wgsl(source.clone().into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let input_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&input),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let size = (expected.len() * 4) as u64;
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.dispatch_workgroups(input.len() as u32, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, size);
        route.renderer.queue().submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap();
            });
        device.poll(wgpu::PollType::wait_indefinitely())?;
        receive.recv()??;
        let bytes = readback.slice(..).get_mapped_range()?;
        let actual: &[u32] = bytemuck::cast_slice(&bytes);
        assert_eq!(
            actual, expected,
            "{backend:?}: cancellation, translation and axis transforms must retain their exact texel coordinates"
        );
    }
    Ok(())
}
