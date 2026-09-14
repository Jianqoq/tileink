//! Independent WGSL reference for native HLSL byte/ABI probes.
use super::{Result, cases::Case, gpu_identity};
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

pub struct Reference {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bindings: wgpu::BindGroupLayout,
    scatter_bindings: wgpu::BindGroupLayout,
    scatter_pipeline: wgpu::ComputePipeline,
    pipelines: BTreeMap<&'static str, wgpu::ComputePipeline>,
}

impl Reference {
    pub fn new(backend: wgpu::Backends, identity: &str) -> Result<Self> {
        Self::with_features(backend, identity, wgpu::Features::empty())
    }
    pub fn with_features(
        backend: wgpu::Backends,
        identity: &str,
        features: wgpu::Features,
    ) -> Result<Self> {
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
                    required_features: features,
                    ..Default::default()
                }))?;
            if gpu_identity::physical_identity(&adapter, &device)? != identity {
                continue;
            }
            let mut entries = [
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
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
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
            let scatter_bindings =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("production range scatter reference bindings"),
                    entries: &[true, false]
                        .into_iter()
                        .enumerate()
                        .map(|(binding, read_only)| wgpu::BindGroupLayoutEntry {
                            binding: binding as u32,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        })
                        .collect::<Vec<_>>(),
                });
            let scatter_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&scatter_bindings)],
                immediate_size: 0,
            });
            let scatter_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("unmodified production range scatter WGSL"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_range_scatter.wgsl"))
                        .into(),
                ),
            });
            let scatter_pipeline =
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: None,
                    layout: Some(&scatter_layout),
                    module: &scatter_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
            return Ok(Self {
                device,
                queue,
                bindings,
                pipelines,
                scatter_bindings,
                scatter_pipeline,
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
        let mut entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(binding, b)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let width = if case.entry == "sample_words" {
            case.params.value[2]
        } else {
            1
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let start = if case.entry == "sample_words" {
            case.params.source_offset as usize
        } else {
            0
        };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &case.source[start..start + width as usize * 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());
        entries.push(wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::TextureView(&view),
        });
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bindings,
            entries: &entries,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipelines[case.entry]);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(case.params.count.div_ceil(64).max(1), 1, 1);
        }
        self.read_output(encoder, &destination, case.destination.len())
    }

    pub fn execute_scatter(&self, case: &super::super::program::Scatter) -> Result<Vec<u8>> {
        let source = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: case.source(),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let destination = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: case.destination(),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.scatter_bindings,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: source.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: destination.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.scatter_pipeline);
            pass.set_bind_group(0, &group, &[]);
            if case.workgroups() != 0 {
                pass.dispatch_workgroups(case.workgroups(), 1, 1);
            }
        }
        self.read_output(encoder, &destination, case.destination().len())
    }

    fn read_output(
        &self,
        mut encoder: wgpu::CommandEncoder,
        destination: &wgpu::Buffer,
        size: usize,
    ) -> Result<Vec<u8>> {
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(destination, 0, &readback, 0, size as u64);
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

#[path = "wgpu/compute.rs"]
mod compute;

#[path = "wgpu/compute_resources.rs"]
mod compute_resources;

#[derive(Clone, Copy, Debug)]
pub struct FilterVariant {
    pub portable: bool,
    pub texture_table: bool,
}
impl FilterVariant {
    fn source(self) -> String {
        let source = if self.portable {
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_filter_web.wgsl"))
        } else {
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_filter.wgsl"))
        };
        // Only the active-tile binding moves. All production algorithms and variants remain intact.
        crate::wgpu::shader_variants::patch_image_resource_shader_source(source, self.texture_table)
            .replace("@binding(52)", "@binding(8)")
    }
}
