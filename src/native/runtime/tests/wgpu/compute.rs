//! Execute production WGSL independently with the same logical input buffers.
use super::Reference;
use crate::native::{
    runtime::{Result, compute::ComputeBatch},
    shaders::BindingKind,
};
use wgpu::util::DeviceExt;
impl Reference {
    pub fn execute_compute(&self, batch: &ComputeBatch) -> Result<Vec<Vec<u8>>> {
        let buffers: Vec<_> = batch
            .buffers()
            .iter()
            .map(|b| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: &b.bytes,
                        usage: wgpu::BufferUsages::STORAGE
                            | wgpu::BufferUsages::UNIFORM
                            | wgpu::BufferUsages::COPY_SRC,
                    })
            })
            .collect();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for stage in batch.passes() {
            let pixel_source;
            let source = match stage.shader.entry {
                "geometry_math_words" | "fill_coverage_words" => {
                    pixel_source = format!(
                        "const FINE_WORKGROUP_SIZE:u32={}u;\n{}\n{}\n{}\n{}",
                        crate::shared::gpu_constants::FINE_WORKGROUP_SIZE,
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/src/wgpu/shaders/shared/pixel.wgsl"
                        )),
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/src/wgpu/shaders/shared/coverage.wgsl"
                        )),
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/src/wgpu/shaders/shared/pattern_transform.wgsl"
                        )),
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tests/shaders/geometry_math.wgsl"
                        ))
                    );
                    pixel_source.as_str()
                }
                "pixel_math_words" => {
                    pixel_source = format!(
                        "const FINE_WORKGROUP_SIZE:u32={}u;\n{}\n{}",
                        crate::shared::gpu_constants::FINE_WORKGROUP_SIZE,
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/src/wgpu/shaders/shared/pixel.wgsl"
                        )),
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tests/shaders/pixel_math.wgsl"
                        ))
                    );
                    pixel_source.as_str()
                }
                "coarse_emit_chunks" | "coarse_emit_chunk_tile_kinds" => include_str!(concat!(
                    env!("OUT_DIR"),
                    "/tileink_wgpu_coarse_emit_web.wgsl"
                )),
                "coarse_emit" | "coarse_emit_bins" => {
                    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_coarse_emit.wgsl"))
                }
                "coarse_count" | "coarse_count_bins" => {
                    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_coarse_count.wgsl"))
                }
                "coarse_emit_chunk_counts"
                | "coarse_emit_prefix_chunks"
                | "coarse_emit_chunk_offsets"
                | "coarse_emit_apply_chunk_offsets"
                | "coarse_emit_fill_refs"
                | "coarse_emit_chunk_particle_counts"
                | "coarse_emit_chunk_particle_offsets"
                | "coarse_tile_counts_from_emit_chunks" => {
                    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_coarse_prefix.wgsl"))
                }
                "coarse_prefix_chunks" | "coarse_chunk_offsets" | "coarse_apply_chunk_offsets" => {
                    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_coarse_prefix.wgsl"))
                }
                "cumsum_prefix_chunks" | "cumsum_chunk_offsets" | "cumsum_apply_chunk_offsets" => {
                    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_cumsum.wgsl"))
                }
                "scan_prefix_chunks" => include_str!(concat!(
                    env!("OUT_DIR"),
                    "/tileink_wgpu_scan_prefix_chunks.wgsl"
                )),
                "scan_chunk_offsets" => include_str!(concat!(
                    env!("OUT_DIR"),
                    "/tileink_wgpu_scan_chunk_offsets.wgsl"
                )),
                "scan_apply_chunk_offsets" => include_str!(concat!(
                    env!("OUT_DIR"),
                    "/tileink_wgpu_scan_apply_chunk_offsets.wgsl"
                )),
                "scan_clear" => {
                    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_scan_clear.wgsl"))
                }
                "scan_count" => {
                    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_scan_count.wgsl"))
                }
                "scan_emit" => {
                    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_scan_emit.wgsl"))
                }
                _ => return Err("missing independent WGSL compute reference".into()),
            };
            // The two production modules both name their entry coarse_emit.
            // Native entries distinguish their tile and chunk dispatch contracts.
            let entry = if stage.shader.entry == "coarse_emit_chunks" {
                "coarse_emit"
            } else {
                stage.shader.entry
            };
            let module = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("production WGSL compute reference"),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                });
            let entries: Vec<_> = stage
                .bindings
                .iter()
                .map(|(b, _)| wgpu::BindGroupLayoutEntry {
                    binding: b.slot,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: match b.kind {
                            BindingKind::Uniform => wgpu::BufferBindingType::Uniform,
                            BindingKind::Read => {
                                wgpu::BufferBindingType::Storage { read_only: true }
                            }
                            BindingKind::Write => {
                                wgpu::BufferBindingType::Storage { read_only: false }
                            }
                        },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                })
                .collect();
            let bindings = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &entries,
                });
            let layout = self
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[Some(&bindings)],
                    immediate_size: 0,
                });
            let pipeline = self
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(stage.shader.entry),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                });
            let entries: Vec<_> = stage
                .bindings
                .iter()
                .map(|(b, id)| wgpu::BindGroupEntry {
                    binding: b.slot,
                    resource: buffers[id.index()].as_entire_binding(),
                })
                .collect();
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &bindings,
                entries: &entries,
            });
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(stage.grid[0], stage.grid[1], stage.grid[2]);
        }
        let mut readbacks = Vec::new();
        for id in batch.outputs() {
            let size = batch.size(*id)? as u64;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(&buffers[id.index()], 0, &readback, 0, size);
            readbacks.push(readback);
        }
        self.queue.submit([encoder.finish()]);
        let mut output = Vec::new();
        for readback in readbacks {
            let (send, receive) = std::sync::mpsc::channel();
            readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = send.send(result);
                });
            self.device.poll(wgpu::PollType::wait_indefinitely())?;
            receive.recv()??;
            output.push(readback.slice(..).get_mapped_range()?.to_vec());
            readback.unmap();
        }
        Ok(output)
    }
}
