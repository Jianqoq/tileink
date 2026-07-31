#![allow(dead_code)] // Shared by the build script and runtime; each side consumes a different subset.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Dx12ResourceClass {
    ConstantBuffer,
    ShaderResource,
    UnorderedAccess,
    Sampler,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Dx12Binding {
    pub(crate) group: u32,
    pub(crate) binding: u32,
    pub(crate) class: Dx12ResourceClass,
    pub(crate) count: u32,
}

pub(crate) const FINE_DXIL_TEXTURE_TABLE_LEN: u32 = 64;
pub(crate) const FINE_DXIL_WORKGROUP_SIZE: (u32, u32, u32) = (256, 1, 1);
pub(crate) const FINE_DXIL_ENTRY_POINTS: [&str; 4] = [
    "fine_tile_main",
    "fine_tile_sdf_list_main",
    "fine_tile_mixed_list_main",
    "fine_tile_full_list_main",
];

// This is the exact binding sequence used by the portable fine pipeline. The build script applies
// wgpu-hal's DX12 register-allocation rules to this list; a layout change must update this contract
// or the precompiled variant is unsafe to use.
pub(crate) const FINE_DXIL_BINDINGS: [Dx12Binding; 12] = [
    binding(0, 0, Dx12ResourceClass::ConstantBuffer, 1),
    binding(0, 1, Dx12ResourceClass::ShaderResource, 1),
    binding(0, 2, Dx12ResourceClass::ShaderResource, 1),
    binding(0, 3, Dx12ResourceClass::ShaderResource, 1),
    binding(0, 4, Dx12ResourceClass::UnorderedAccess, 1),
    binding(0, 5, Dx12ResourceClass::ShaderResource, 1),
    binding(0, 6, Dx12ResourceClass::ShaderResource, 1),
    binding(0, 7, Dx12ResourceClass::UnorderedAccess, 1),
    binding(0, 8, Dx12ResourceClass::UnorderedAccess, 1),
    binding(1, 0, Dx12ResourceClass::ShaderResource, 1),
    binding(1, 1, Dx12ResourceClass::Sampler, 1),
    binding(
        1,
        2,
        Dx12ResourceClass::ShaderResource,
        FINE_DXIL_TEXTURE_TABLE_LEN,
    ),
];

const fn binding(group: u32, binding: u32, class: Dx12ResourceClass, count: u32) -> Dx12Binding {
    Dx12Binding {
        group,
        binding,
        class,
        count,
    }
}
