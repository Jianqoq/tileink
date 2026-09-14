use wgpu::util::DeviceExt;

const SIZE: u32 = 64;
const ROW_BYTES: u32 = SIZE * 4;

// The real DX12 command encoder must order write-only dispatches even though the
// texture remains in UNORDERED_ACCESS. A readback checks GPU execution, not just
// the CPU usage flags that previously missed this dependency.
pub struct TextureWrites {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    texture: wgpu::Texture,
    pipeline: wgpu::ComputePipeline,
    bindings: [wgpu::BindGroup; 2],
}

impl TextureWrites {
    pub fn new() -> Self {
        let dxc_path = std::env::var("TILEINK_PARITY_DXCOMPILER")
            .expect("set TILEINK_PARITY_DXCOMPILER to an absolute dxcompiler.dll path");
        assert!(std::path::Path::new(&dxc_path).is_absolute());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            backend_options: wgpu::BackendOptions {
                dx12: wgpu::Dx12BackendOptions {
                    shader_compiler: wgpu::Dx12Compiler::DynamicDxc { dxc_path },
                    ..Default::default()
                },
                ..Default::default()
            },
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .expect("DX12 hardware adapter is required; no backend fallback");
        assert_eq!(adapter.get_info().backend, wgpu::Backend::Dx12);
        assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
        eprintln!("texture ordering adapter: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            // Write-only storage is required to work without optional read/write support.
            required_features: wgpu::Features::empty(),
            ..Default::default()
        }))
        .unwrap();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("write-only ordering target"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("write-only ordering shader"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@group(0) @binding(0) var output_image: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(1) var<uniform> color: vec4<f32>;
@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    textureStore(output_image, vec2<i32>(id.xy), color);
}
"#
                .into(),
            ),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("write-only ordering pipeline"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let view = texture.create_view(&Default::default());
        let bindings = [[1.0_f32; 4], [0.0; 4]].map(|color| {
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("write-only ordering color"),
                contents: bytemuck::cast_slice(&color),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("write-only ordering bindings"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            })
        });
        Self {
            device,
            queue,
            texture,
            pipeline,
            bindings,
        }
    }

    pub fn encode(&self, pairs: u32) -> wgpu::CommandEncoder {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for _ in 0..pairs {
            for binding in &self.bindings {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, binding, &[]);
                pass.dispatch_workgroups(SIZE / 16, SIZE / 16, 1);
            }
        }
        encoder
    }

    pub fn assert_cleared(&self, pairs: u32, frame: u32) {
        let mut encoder = self.encode(pairs);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("write-only ordering readback"),
            size: u64::from(ROW_BYTES * SIZE),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ROW_BYTES),
                    rows_per_image: Some(SIZE),
                },
            },
            self.texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |result| {
            tx.send(result).unwrap()
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let mapped = readback.get_mapped_range(..).unwrap();
        let changed = mapped
            .chunks_exact(4)
            .filter(|rgba| *rgba != [0, 0, 0, 0])
            .count();
        assert_eq!(
            changed, 0,
            "frame {frame}, {pairs} write pairs: last write must win in every RGBA channel"
        );
    }
}
