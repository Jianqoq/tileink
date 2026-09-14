//! Execute production WGSL independently with the same logical input buffers.
use super::compute_resources::{self, GpuResource};
use super::{FilterVariant, FineVariant, Reference};
use crate::native::runtime::{Result, compute::ComputeBatch};
impl Reference {
    pub fn execute_compute(&self, batch: &ComputeBatch) -> Result<Vec<Vec<u8>>> {
        self.execute_variant(batch, None)
    }
    pub fn execute_variant(
        &self,
        batch: &ComputeBatch,
        filter: Option<FilterVariant>,
    ) -> Result<Vec<Vec<u8>>> {
        self.execute_selected(batch, filter, None)
    }
    pub fn execute_selected(
        &self,
        batch: &ComputeBatch,
        filter: Option<FilterVariant>,
        fine: Option<FineVariant>,
    ) -> Result<Vec<Vec<u8>>> {
        let resources: Vec<_> = batch
            .resources()
            .iter()
            .map(|input| GpuResource::new(&self.device, &self.queue, input))
            .collect();
        let tables: Vec<Option<Vec<&wgpu::TextureView>>> = batch
            .resources()
            .iter()
            .map(|resource| {
                if let crate::native::runtime::compute::Resource::TextureTable(images) = resource {
                    Some(
                        images
                            .iter()
                            .map(|id| {
                                let GpuResource::Texture(_, view) = &resources[id.index()] else {
                                    unreachable!("validated table image")
                                };
                                view
                            })
                            .collect(),
                    )
                } else {
                    None
                }
            })
            .collect();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for stage in batch.passes() {
            let fine_portable =
                stage.shader.entry == "fine_tile_main" && fine.is_some_and(|v| v.portable);
            let helper_source;
            let source = match stage.shader.entry {
                "sdf_coverage_words" => {
                    helper_source = format!(
                        "{}\nconst SDF_PROBE_REQUEST_WORDS:u32={}u;\nconst SDF_PROBE_AFFINE_WORD:u32={}u;\n{}",
                        filter
                            .ok_or("SDF reference variant must be explicit")?
                            .source()
                            .replace("@binding(10)", "@binding(7)"),
                        crate::shared::gpu_constants::SDF_PROBE_REQUEST_WORDS,
                        crate::shared::gpu_constants::SDF_PROBE_AFFINE_WORD,
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tests/shaders/sdf.wgsl"
                        ))
                    );
                    helper_source.as_str()
                }
                entry if entry.starts_with("filter_") => {
                    helper_source = filter
                        .ok_or("filter reference variant must be explicit")?
                        .source();
                    helper_source.as_str()
                }
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
                "sampler_words" => {
                    helper_source = format!(
                        "const FINE_WORKGROUP_SIZE:u32={}u;\n{}\n{}",
                        crate::shared::gpu_constants::FINE_WORKGROUP_SIZE,
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/src/wgpu/shaders/shared/pixel.wgsl"
                        )),
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tests/shaders/sampler.wgsl"
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
                "texture_table_words" => {
                    helper_source = format!(
                        "enable wgpu_binding_array;\nconst NATIVE_TEXTURE_TABLE_CAPACITY:u32={}u;\nconst FINE_WORKGROUP_SIZE:u32={}u;\n{}\n{}",
                        crate::shared::gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY,
                        crate::shared::gpu_constants::FINE_WORKGROUP_SIZE,
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/src/wgpu/shaders/shared/pixel.wgsl"
                        )),
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tests/shaders/texture_table.wgsl"
                        ))
                    );
                    helper_source.as_str()
                }
                "texture_layer" => {
                    helper_source = format!(
                        "const FINE_WORKGROUP_SIZE:u32={}u;\n{}",
                        crate::shared::gpu_constants::FINE_WORKGROUP_SIZE,
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tests/shaders/texture_array.wgsl"
                        ))
                    );
                    helper_source.as_str()
                }
                "pattern_words" => {
                    // The production atlas functions are unchanged; only bind-group locations
                    // are relocated to the explicit flat native interface for this reference.
                    let production =
                        crate::wgpu::shader_variants::patch_image_resource_shader_source(
                            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_fine_web.wgsl")),
                            false,
                        )
                        .replace("@group(1) @binding(0)", "@group(0) @binding(12)")
                        .replace("@group(1) @binding(1)", "@group(0) @binding(13)");
                    helper_source = format!(
                        "{}\n{}",
                        production,
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tests/shaders/pattern.wgsl"
                        ))
                    );
                    helper_source.as_str()
                }
                "brush_words" => {
                    let production =
                        crate::wgpu::shader_variants::patch_image_resource_shader_source(
                            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_fine_web.wgsl")),
                            true,
                        )
                        .replace("@group(1) @binding(0)", "@group(0) @binding(12)")
                        .replace("@group(1) @binding(1)", "@group(0) @binding(13)")
                        .replace("@group(1) @binding(2)", "@group(1) @binding(30)");
                    helper_source = format!(
                        "{}\n{}",
                        production,
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tests/shaders/brush.wgsl"
                        ))
                    );
                    helper_source.as_str()
                }
                "fine_tile_main" => {
                    helper_source = fine
                        .ok_or("fine reference variant must be explicit")?
                        .source();
                    helper_source.as_str()
                }
                "text_words" => {
                    helper_source = format!(
                        "{}\n{}",
                        crate::wgpu::shader_variants::patch_image_resource_shader_source(
                            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_fine_web.wgsl")),
                            false
                        ),
                        include_str!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tests/shaders/text.wgsl"
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
            let mut entries: Vec<_> = stage
                .bindings
                .iter()
                .map(|(b, _)| wgpu::BindGroupLayoutEntry {
                    binding: b.slot,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: if fine_portable && b.slot == 1 {
                        compute_resources::layout(crate::native::shaders::BindingKind::Texture)
                    } else if (stage.shader.entry == "fine_tile_main" && b.slot == 1)
                        || (stage.shader.entry.starts_with("filter_")
                            && b.slot == 3
                            && filter.is_some_and(|v| !v.portable))
                    {
                        wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::ReadWrite,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        }
                    } else {
                        compute_resources::layout(b.kind)
                    },
                    count: if b.kind == crate::native::shaders::BindingKind::TextureTable {
                        std::num::NonZeroU32::new(b.count)
                    } else {
                        None
                    },
                })
                .collect();
            let snapshot = if fine_portable {
                let target = stage.bindings.iter().find(|(b, _)| b.slot == 1).unwrap().1;
                entries.push(wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: compute_resources::layout(
                        crate::native::shaders::BindingKind::TextureWrite,
                    ),
                    count: None,
                });
                Some(resources[target.index()].snapshot(&self.device, &mut encoder))
            } else if filter.is_some_and(|v| v.portable)
                && matches!(
                    stage.shader.entry,
                    "filter_composite_drop_shadow_region"
                        | "filter_source_over_region"
                        | "filter_color_region"
                        | "filter_color_matrix_region"
                        | "filter_apply_region_mask"
                        | "filter_component_transfer_region"
                        | "filter_composite_direct_region"
                        | "filter_composite_surface_direct_region"
                        | "filter_composite_stack_region"
                        | "filter_liquid_glass_rect_composite_region"
                        | "filter_composite_blend_stack_region"
                        | "filter_composite_surface_stack_region"
                        | "filter_composite_rect_direct_region"
                        | "filter_upsample_rect_composite_region"
                )
            {
                let target = stage.bindings.iter().find(|(b, _)| b.slot == 3).unwrap().1;
                let snapshot = resources[target.index()].snapshot(&self.device, &mut encoder);
                entries.push(wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: compute_resources::layout(crate::native::shaders::BindingKind::Texture),
                    count: None,
                });
                Some(snapshot)
            } else {
                None
            };
            // wgpu forbids uniform buffers in a bind group containing a binding array.
            // Keep table bindings in a separate group without changing shader algorithms.
            let table_slots: Vec<_> = stage
                .bindings
                .iter()
                .filter(|(b, _)| b.kind == crate::native::shaders::BindingKind::TextureTable)
                .map(|(b, _)| b.slot)
                .collect();
            let compiled =
                self.compute_cache
                    .pipeline(&self.device, source, entry, &entries, &table_slots);
            let layouts = compiled.layouts;
            let pipeline = compiled.pipeline;
            let mut entries: Vec<_> = stage
                .bindings
                .iter()
                .map(|(b, id)| wgpu::BindGroupEntry {
                    binding: b.slot,
                    resource: if fine_portable && b.slot == 1 {
                        snapshot.as_ref().unwrap().binding()
                    } else if let Some(views) = &tables[id.index()] {
                        wgpu::BindingResource::TextureViewArray(views)
                    } else {
                        resources[id.index()].binding()
                    },
                })
                .collect();
            if let Some(snapshot) = &snapshot {
                let (binding, resource) = if fine_portable {
                    let target = stage.bindings.iter().find(|(b, _)| b.slot == 1).unwrap().1;
                    (8, resources[target.index()].binding())
                } else {
                    (9, snapshot.binding())
                };
                entries.push(wgpu::BindGroupEntry { binding, resource });
            }
            let groups: Vec<_> = layouts
                .iter()
                .enumerate()
                .map(|(group, layout)| {
                    let entries: Vec<_> = entries
                        .iter()
                        .filter(|e| usize::from(table_slots.contains(&e.binding)) == group)
                        .cloned()
                        .collect();
                    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: None,
                        layout,
                        entries: &entries,
                    })
                })
                .collect();
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            for (index, group) in groups.iter().enumerate() {
                pass.set_bind_group(index as u32, group, &[]);
            }
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
