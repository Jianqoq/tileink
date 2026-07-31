use std::borrow::Cow;

use super::dxil_manifest::FINE_DXIL_TEXTURE_TABLE_LEN;

#[derive(Clone, Copy)]
pub(crate) struct PrecompiledDxil {
    pub(crate) entry_point: &'static str,
    pub(crate) workgroup_size: (u32, u32, u32),
    pub(crate) bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/tileink_dxil.rs"));

#[derive(Clone, Copy)]
pub(crate) struct FineDxilSet {
    shaders: &'static [PrecompiledDxil],
}

impl FineDxilSet {
    pub(crate) fn for_entry_point(self, entry_point: &str) -> Option<PrecompiledDxil> {
        self.shaders
            .iter()
            .copied()
            .find(|shader| shader.entry_point == entry_point)
    }
}

pub(crate) fn fine_dxil_set(
    device: &::wgpu::Device,
    portable_textures: bool,
    large_texture_table_len: u32,
) -> FineDxilSet {
    let supported = fine_dxil_supported(
        device.adapter_info().backend,
        device.features(),
        portable_textures,
        large_texture_table_len,
        dx12_shader_model_6_supported(device),
        !PRECOMPILED_FINE_DXIL.is_empty(),
    );
    FineDxilSet {
        shaders: if supported {
            PRECOMPILED_FINE_DXIL
        } else {
            &[]
        },
    }
}

fn fine_dxil_supported(
    backend: ::wgpu::Backend,
    features: ::wgpu::Features,
    portable_textures: bool,
    large_texture_table_len: u32,
    shader_model_6_supported: bool,
    artifacts_available: bool,
) -> bool {
    artifacts_available
        && backend == ::wgpu::Backend::Dx12
        && features.contains(::wgpu::Features::PASSTHROUGH_SHADERS)
        && portable_textures
        && large_texture_table_len == FINE_DXIL_TEXTURE_TABLE_LEN
        && shader_model_6_supported
}

#[cfg(target_os = "windows")]
fn dx12_shader_model_6_supported(device: &::wgpu::Device) -> bool {
    use windows::Win32::Graphics::Direct3D12::{
        D3D_SHADER_MODEL_6_0, D3D12_FEATURE_DATA_SHADER_MODEL, D3D12_FEATURE_SHADER_MODEL,
    };

    // WGPU's passthrough feature permits caller-owned bytecode but does not describe that
    // bytecode's required shader model. Querying the actual D3D12 device fixes the root safety
    // issue: an SM 5.1 adapter must take the validated WGSL path before unsafe module creation.
    let Some(device) = (unsafe { device.as_hal::<::wgpu::hal::api::Dx12>() }) else {
        return false;
    };
    let mut support = D3D12_FEATURE_DATA_SHADER_MODEL {
        HighestShaderModel: D3D_SHADER_MODEL_6_0,
    };
    unsafe {
        device.raw_device().CheckFeatureSupport(
            D3D12_FEATURE_SHADER_MODEL,
            std::ptr::from_mut(&mut support).cast(),
            std::mem::size_of_val(&support) as u32,
        )
    }
    .is_ok()
        && support.HighestShaderModel.0 >= D3D_SHADER_MODEL_6_0.0
}

#[cfg(not(target_os = "windows"))]
fn dx12_shader_model_6_supported(_: &::wgpu::Device) -> bool {
    false
}

impl PrecompiledDxil {
    pub(crate) fn create_shader_module(
        self,
        device: &::wgpu::Device,
        label: &'static str,
    ) -> ::wgpu::ShaderModule {
        let descriptor = ::wgpu::ShaderModuleDescriptorPassthrough {
            label: Some(label),
            entry_points: Cow::Owned(vec![::wgpu::PassthroughShaderEntryPoint {
                name: Cow::Borrowed(self.entry_point),
                workgroup_size: self.workgroup_size,
            }]),
            dxil: Some(Cow::Borrowed(self.bytes)),
            ..Default::default()
        };
        // SAFETY: build/dxil.rs compiles this library's validated WGSL with the same Naga HLSL
        // options and register-allocation contract as wgpu-hal DX12. Selection above requires the
        // exact backend, SM 6.0 capability, feature, portable target, and 64-entry texture-table
        // layout for that blob. This is the root capability gate, not a failed-pipeline workaround.
        unsafe { device.create_shader_module_passthrough(descriptor) }
    }
}

#[cfg(test)]
mod tests {
    use super::{FINE_DXIL_TEXTURE_TABLE_LEN, fine_dxil_supported};
    use crate::wgpu::dxil_manifest::FINE_DXIL_ENTRY_POINTS;

    #[test]
    fn precompiled_dxil_requires_the_exact_dx12_layout_contract() {
        let features = ::wgpu::Features::PASSTHROUGH_SHADERS;
        assert!(fine_dxil_supported(
            ::wgpu::Backend::Dx12,
            features,
            true,
            FINE_DXIL_TEXTURE_TABLE_LEN,
            true,
            true,
        ));
        assert!(!fine_dxil_supported(
            ::wgpu::Backend::Vulkan,
            features,
            true,
            FINE_DXIL_TEXTURE_TABLE_LEN,
            true,
            true,
        ));
        assert!(!fine_dxil_supported(
            ::wgpu::Backend::Dx12,
            ::wgpu::Features::empty(),
            true,
            FINE_DXIL_TEXTURE_TABLE_LEN,
            true,
            true,
        ));
        assert!(!fine_dxil_supported(
            ::wgpu::Backend::Dx12,
            features,
            false,
            FINE_DXIL_TEXTURE_TABLE_LEN,
            true,
            true,
        ));
        assert!(!fine_dxil_supported(
            ::wgpu::Backend::Dx12,
            features,
            true,
            FINE_DXIL_TEXTURE_TABLE_LEN - 1,
            true,
            true,
        ));
        assert!(!fine_dxil_supported(
            ::wgpu::Backend::Dx12,
            features,
            true,
            FINE_DXIL_TEXTURE_TABLE_LEN,
            false,
            true,
        ));
        assert!(!fine_dxil_supported(
            ::wgpu::Backend::Dx12,
            features,
            true,
            FINE_DXIL_TEXTURE_TABLE_LEN,
            true,
            false,
        ));
    }

    #[test]
    fn embedded_dxil_is_complete_when_available() {
        if super::PRECOMPILED_FINE_DXIL.is_empty() {
            return;
        }
        assert_eq!(
            super::PRECOMPILED_FINE_DXIL.len(),
            FINE_DXIL_ENTRY_POINTS.len()
        );
        for entry_point in FINE_DXIL_ENTRY_POINTS {
            let shader = super::PRECOMPILED_FINE_DXIL
                .iter()
                .find(|shader| shader.entry_point == entry_point)
                .unwrap_or_else(|| panic!("missing precompiled DXIL entry point {entry_point}"));
            assert!(!shader.bytes.is_empty());
        }
    }
}
