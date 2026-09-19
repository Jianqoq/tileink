//! Explicit storage ordering for upstream wgpu's DX12 texture barrier gap.
//! This is a consumer-side workaround, not a HAL fork. Remove it once upstream
//! orders every same-UAV-state storage dependency (including write-only writes).

pub(crate) fn prepare_write(encoder: &mut ::wgpu::CommandEncoder, texture: &::wgpu::Texture) {
    // Both storage uses map to UNORDERED_ACCESS. Upstream emits the required UAV
    // barrier when the *previous* tracked use is STORAGE_READ_WRITE. Establish
    // that state before the write-only pass so its automatic transition orders
    // all earlier UAV accesses, including writes from another command buffer.
    // This changes only synchronization, not shader bindings or device features.
    encoder.transition_resources(
        std::iter::empty(),
        std::iter::once(::wgpu::TextureTransition {
            texture,
            selector: None,
            state: ::wgpu::TextureUses::STORAGE_READ_WRITE,
        }),
    );
}
