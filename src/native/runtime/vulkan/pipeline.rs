//! Vulkan descriptor layouts, SPIR-V modules and driver pipeline cache.
use super::*;
use std::ffi::CString;

pub(super) fn create(this: &mut Vulkan, physical: vk::PhysicalDevice) -> Result<()> {
    unsafe {
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(3)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
        ];
        this.bindings = this.device.create_descriptor_set_layout(
            &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
            None,
        )?;
        let layouts = [this.bindings];
        this.layout = this.device.create_pipeline_layout(
            &vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts),
            None,
        )?;
        for artifact in crate::NATIVE_SHADER_ARTIFACTS
            .iter()
            .filter(|a| a.format == "spirv" && a.bindings.is_empty())
        {
            let properties = this.instance.get_physical_device_properties(physical);
            super::limits::workgroup(
                artifact.workgroup,
                properties.limits.max_compute_work_group_size,
                properties.limits.max_compute_work_group_invocations,
            )?;
            let words: Vec<u32> = artifact
                .bytes
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            let shader = this
                .device
                .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)?;
            let name = CString::new(artifact.entry)?;
            let stage = vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::COMPUTE)
                .module(shader)
                .name(&name);
            let identity = serde_json::to_vec(
                &serde_json::json!({"api":"vulkan","version":properties.api_version,"vendor":properties.vendor_id,"device":properties.device_id,"driver":properties.driver_version,"uuid":properties.pipeline_cache_uuid}),
            )?;
            let pipeline = std::cell::Cell::new(vk::Pipeline::null());
            let build = |data: &[u8]| -> std::result::Result<(vk::Pipeline, Vec<u8>), vk::Result> {
                let cache = this.device.create_pipeline_cache(
                    &vk::PipelineCacheCreateInfo::default().initial_data(data),
                    None,
                )?;
                let result = this.device.create_compute_pipelines(
                    cache,
                    &[vk::ComputePipelineCreateInfo::default()
                        .stage(stage)
                        .layout(this.layout)],
                    None,
                );
                let handle = match result {
                    Ok(p) => p[0],
                    Err((partial, error)) => {
                        for p in partial {
                            this.device.destroy_pipeline(p, None);
                        }
                        this.device.destroy_pipeline_cache(cache, None);
                        return Err(error);
                    }
                };
                let bytes = this.device.get_pipeline_cache_data(cache);
                this.device.destroy_pipeline_cache(cache, None);
                match bytes {
                    Ok(bytes) => Ok((handle, bytes)),
                    Err(error) => {
                        this.device.destroy_pipeline(handle, None);
                        Err(error)
                    }
                }
            };
            let cached = super::super::pipeline_cache::load_or_create(
                &identity,
                artifact.cache_key,
                |data| {
                    if !cache_header_matches(
                        data,
                        properties.vendor_id,
                        properties.device_id,
                        &properties.pipeline_cache_uuid,
                    ) {
                        return Ok(false);
                    }
                    match build(data) {
                        Ok((handle, _)) => {
                            pipeline.set(handle);
                            Ok(true)
                        }
                        Err(_) => Ok(false),
                    }
                },
                || {
                    let (handle, bytes) = build(&[]).map_err(std::io::Error::other)?;
                    pipeline.set(handle);
                    Ok(bytes)
                },
            );
            this.device.destroy_shader_module(shader, None);
            match cached {
                Ok(hit) => {
                    eprintln!(
                        "native Vulkan pipeline {}: {}",
                        artifact.entry,
                        if hit { "cache hit" } else { "compiled" }
                    );
                    this.pipelines.insert(artifact.entry, pipeline.get());
                }
                Err(error) => {
                    this.device.destroy_pipeline(pipeline.get(), None);
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }
}

pub(super) fn cache_header_matches(
    bytes: &[u8],
    vendor: u32,
    device: u32,
    uuid: &[u8; 16],
) -> bool {
    bytes.len() >= 32
        && u32::from_le_bytes(bytes[0..4].try_into().unwrap()) == 32
        && u32::from_le_bytes(bytes[4..8].try_into().unwrap()) == 1
        && u32::from_le_bytes(bytes[8..12].try_into().unwrap()) == vendor
        && u32::from_le_bytes(bytes[12..16].try_into().unwrap()) == device
        && bytes[16..32] == *uuid
}
