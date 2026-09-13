//! Buffer compute layouts and lazily cached native SPIR-V pipelines.
use super::super::Result;
use crate::native::shaders::BindingKind;
use ash::vk;
use std::{collections::BTreeMap, ffi::CString};

pub struct Pipeline {
    device: ash::Device,
    pub bindings: vk::DescriptorSetLayout,
    pub layout: vk::PipelineLayout,
    pub pipeline: vk::Pipeline,
}
pub fn descriptor(kind: BindingKind) -> vk::DescriptorType {
    match kind {
        BindingKind::Uniform => vk::DescriptorType::UNIFORM_BUFFER,
        BindingKind::Read | BindingKind::Write => vk::DescriptorType::STORAGE_BUFFER,
    }
}
pub fn ensure(
    device: &ash::Device,
    properties: &vk::PhysicalDeviceProperties,
    pipelines: &mut BTreeMap<&'static str, Pipeline>,
    entry: &str,
) -> Result<()> {
    if pipelines.contains_key(entry) {
        return Ok(());
    }
    let artifact = crate::NATIVE_SHADER_ARTIFACTS
        .iter()
        .find(|a| a.format == "spirv" && a.entry == entry && !a.bindings.is_empty())
        .ok_or("missing native SPIR-V compute artifact")?;
    super::limits::workgroup(
        artifact.workgroup,
        properties.limits.max_compute_work_group_size,
        properties.limits.max_compute_work_group_invocations,
    )?;
    unsafe {
        let mut result = Pipeline {
            device: device.clone(),
            bindings: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
        };
        let bindings: Vec<_> = artifact
            .bindings
            .iter()
            .map(|b| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(b.slot)
                    .descriptor_type(descriptor(b.kind))
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE)
            })
            .collect();
        result.bindings = device.create_descriptor_set_layout(
            &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
            None,
        )?;
        result.layout = device.create_pipeline_layout(
            &vk::PipelineLayoutCreateInfo::default().set_layouts(&[result.bindings]),
            None,
        )?;
        let name = CString::new(entry)?;
        let words: Vec<_> = artifact
            .bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let shader = device
            .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)?;
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader)
            .name(&name);
        let identity = serde_json::to_vec(
            &serde_json::json!({"api":"vulkan","version":properties.api_version,"vendor":properties.vendor_id,"device":properties.device_id,"driver":properties.driver_version,"uuid":properties.pipeline_cache_uuid}),
        );
        let cached = (|| -> Result<bool> {
            let identity = identity?;
            let handle = std::cell::Cell::new(vk::Pipeline::null());
            let build = |data: &[u8]| -> std::result::Result<(vk::Pipeline, Vec<u8>), vk::Result> {
                let cache = device.create_pipeline_cache(
                    &vk::PipelineCacheCreateInfo::default().initial_data(data),
                    None,
                )?;
                let created = device.create_compute_pipelines(
                    cache,
                    &[vk::ComputePipelineCreateInfo::default()
                        .stage(stage)
                        .layout(result.layout)],
                    None,
                );
                let pipeline = match created {
                    Ok(p) => p[0],
                    Err((partial, error)) => {
                        for p in partial {
                            device.destroy_pipeline(p, None);
                        }
                        device.destroy_pipeline_cache(cache, None);
                        return Err(error);
                    }
                };
                let bytes = device.get_pipeline_cache_data(cache);
                device.destroy_pipeline_cache(cache, None);
                match bytes {
                    Ok(bytes) => Ok((pipeline, bytes)),
                    Err(e) => {
                        device.destroy_pipeline(pipeline, None);
                        Err(e)
                    }
                }
            };
            let loaded = super::super::pipeline_cache::load_or_create_for_layout(
                &identity,
                artifact.cache_key,
                b"native-compute-buffer-table-v1",
                |data| {
                    if !super::pipeline::cache_header_matches(
                        data,
                        properties.vendor_id,
                        properties.device_id,
                        &properties.pipeline_cache_uuid,
                    ) {
                        return Ok(false);
                    }
                    match build(data) {
                        Ok((p, _)) => {
                            handle.set(p);
                            Ok(true)
                        }
                        Err(_) => Ok(false),
                    }
                },
                || {
                    let (p, bytes) = build(&[]).map_err(std::io::Error::other)?;
                    handle.set(p);
                    Ok(bytes)
                },
            );
            result.pipeline = handle.get();
            Ok(loaded?)
        })();
        device.destroy_shader_module(shader, None);
        let hit = cached?;
        eprintln!(
            "native Vulkan compute {entry}: {}",
            if hit { "cache hit" } else { "compiled" }
        );
        pipelines.insert(artifact.entry, result);
        Ok(())
    }
}
impl Drop for Pipeline {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_pipeline(self.pipeline, None);
            self.device.destroy_pipeline_layout(self.layout, None);
            self.device
                .destroy_descriptor_set_layout(self.bindings, None);
        }
    }
}
