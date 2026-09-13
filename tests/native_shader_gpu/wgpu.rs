//! Independent WGSL reference for native HLSL byte/ABI probes.
use super::{Result, cases::Case, gpu_identity};
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

pub struct Reference {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bindings: wgpu::BindGroupLayout,
    pipelines: BTreeMap<&'static str, wgpu::ComputePipeline>,
}

impl Reference {
    pub fn new(backend: wgpu::Backends, identity: &str) -> Result<Self> {
        let mut descriptor = wgpu::InstanceDescriptor {
            backends: backend,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        };
        if backend == wgpu::Backends::DX12 {
            descriptor.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::DynamicDxc {
                dxc_path: std::env::var("TILEINK_PARITY_DXCOMPILER")?,
            };
        }
        let instance = wgpu::Instance::new(descriptor);
        for adapter in pollster::block_on(instance.enumerate_adapters(backend)) {
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    required_limits: adapter.limits(),
                    ..Default::default()
                }))?;
            if gpu_identity::physical_identity(&adapter, &device)? != identity {
                continue;
            }
            let entries = [
                wgpu::BufferBindingType::Storage { read_only: false },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Uniform,
            ]
            .into_iter()
            .enumerate()
            .map(|(binding, ty)| wgpu::BindGroupLayoutEntry {
                binding: binding as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect::<Vec<_>>();
            let bindings = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("native probe reference ABI"),
                entries: &entries,
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&bindings)],
                immediate_size: 0,
            });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("independent native ABI reference"),
                source: wgpu::ShaderSource::Wgsl(include_str!("reference.wgsl").into()),
            });
            let pipelines = ["clear_words", "copy_words", "layout_words", "sample_words"]
                .into_iter()
                .map(|entry| {
                    (
                        entry,
                        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                            label: Some(entry),
                            layout: Some(&layout),
                            module: &module,
                            entry_point: Some(entry),
                            compilation_options: Default::default(),
                            cache: None,
                        }),
                    )
                })
                .collect();
            return Ok(Self {
                device,
                queue,
                bindings,
                pipelines,
            });
        }
        Err("requested wgpu reference physical GPU unavailable".into())
    }

    pub fn execute(&self, case: &Case) -> Result<Vec<u8>> {
        let destination = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &case.destination,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        let source = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &case.source,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let params = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::bytes_of(&case.params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let buffers = [&destination, &source, &params];
        let entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(binding, b)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bindings,
            entries: &entries,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: case.destination.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipelines[case.entry]);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(case.params.count.div_ceil(64).max(1), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&destination, 0, &readback, 0, case.destination.len() as u64);
        self.queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        receive.recv()??;
        let output = readback.slice(..).get_mapped_range()?.to_vec();
        readback.unmap();
        Ok(output)
    }
}
