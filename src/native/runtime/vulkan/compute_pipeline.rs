//! Buffer compute layouts and lazily cached native SPIR-V pipelines.
use super::super::{Result, pipeline_cache};
mod cache;
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
        BindingKind::Sampler => vk::DescriptorType::SAMPLER,
        BindingKind::Uniform => vk::DescriptorType::UNIFORM_BUFFER,
        BindingKind::Texture | BindingKind::TextureArray | BindingKind::TextureTable => {
            vk::DescriptorType::SAMPLED_IMAGE
        }
        BindingKind::TextureWrite | BindingKind::TextureReadWrite => {
            vk::DescriptorType::STORAGE_IMAGE
        }
        BindingKind::Read | BindingKind::Write => vk::DescriptorType::STORAGE_BUFFER,
    }
}
pub fn ensure(
    device: &ash::Device,
    properties: &vk::PhysicalDeviceProperties,
    pipelines: &mut BTreeMap<String, Pipeline>,
    pass: &crate::native::runtime::compute::Pass,
) -> Result<()> {
    let entry = pass.entry();
    if pipelines.contains_key(pass.key()) {
        return Ok(());
    }
    let artifact = if pass.native_source.is_some() {
        pass.artifact()
    } else {
        crate::NATIVE_SHADER_ARTIFACTS
            .iter()
            .find(|a| a.format == "spirv" && a.entry == entry && !a.bindings.is_empty())
            .ok_or("missing native SPIR-V compute artifact")?
            .to_owned()
    };
    let compiled = pass
        .native_source
        .as_ref()
        .map(|source| crate::native::runtime::program_shader::dxc::compile(source, "spirv"))
        .transpose()?;
    let (bytes, key) = compiled
        .as_ref()
        .map_or((artifact.bytes, artifact.cache_key), |s| {
            (s.bytes.as_slice(), s.key.as_str())
        });
    super::limits::descriptors(artifact.bindings, &properties.limits)?;
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
                    .descriptor_count(b.count)
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
        let words: Vec<_> = bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let shader = device
            .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)?;
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader)
            .name(&name);
        let cached = cache::create(device, properties, key, stage, result.layout);
        device.destroy_shader_module(shader, None);
        let (pipeline, hit) = cached?;
        result.pipeline = pipeline;
        eprintln!(
            "native Vulkan compute {entry}: {}",
            if hit { "cache hit" } else { "compiled" }
        );
        pipelines.insert(pass.key().to_owned(), result);
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
