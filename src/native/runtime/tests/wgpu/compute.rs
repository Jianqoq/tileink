//! Execute production WGSL independently with the same logical input buffers.
use super::Reference;
use super::compute_resources::{self, GpuResource};
use crate::native::runtime::{Result, compute::ComputeBatch};
impl Reference {
    pub fn execute_compute(&self, batch: &ComputeBatch) -> Result<Vec<Vec<u8>>> {
        let resources: Vec<_> = batch
            .resources()
            .iter()
            .map(|input| GpuResource::new(&self.device, &self.queue, input))
            .collect();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for stage in batch.passes() {
            let helper_source;
            let source = match stage.shader.entry {
                "geometry_math_words" | "fill_coverage_words" => {
                    helper_source = format!(
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
                    helper_source.as_str()
                }
                "texture_flip" => {
                    helper_source = format!(
                        "const FINE_WORKGROUP_SIZE:u32={}u;\n{}",
                        crate::shared::gpu_constants::FINE_WORKGROUP_SIZE,
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tests/shaders/texture.wgsl"
                        ))
                    );
                    helper_source.as_str()
                }
                "gradient_words" => {
                    helper_source = format!(
                        "{}\n{}",
                        crate::wgpu::shader_variants::patch_image_resource_shader_source(
                            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_fine_web.wgsl")),
                            false
                        ),
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tests/shaders/gradient.wgsl"
                        ))
                    );
                    helper_source.as_str()
                }
                "blend_math_words" => {
                    helper_source = format!(
                        "const FINE_WORKGROUP_SIZE:u32={}u;\n{}\n{}\n{}",
                        crate::shared::gpu_constants::FINE_WORKGROUP_SIZE,
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/src/wgpu/shaders/shared/pixel.wgsl"
                        )),
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/src/wgpu/shaders/shared/blend.wgsl"
                        )),
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tests/shaders/blend_math.wgsl"
                        ))
                    );
                    helper_source.as_str()
                }
                "pixel_math_words" => {
                    helper_source = format!(
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
                    helper_source.as_str()
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
                    ty: compute_resources::layout(b.kind),
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
                    resource: resources[id.index()].binding(),
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
        let readbacks: Vec<_> = batch
            .outputs()
            .iter()
            .map(|id| resources[id.index()].readback(&self.device, &mut encoder))
            .collect();
        self.queue.submit([encoder.finish()]);
        let mut output = Vec::new();
        for readback in readbacks {
            let (send, receive) = std::sync::mpsc::channel();
            readback
                .buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = send.send(result);
                });
            self.device.poll(wgpu::PollType::wait_indefinitely())?;
            receive.recv()??;
            output.push(readback.unpack(&readback.buffer.slice(..).get_mapped_range()?));
            readback.buffer.unmap();
        }
        Ok(output)
    }
}
