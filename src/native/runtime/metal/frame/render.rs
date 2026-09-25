//! Fine rendering loads only when existing pixels must survive the render pass.
use super::*;
use crate::{
    native::runtime::compute::{Pass, Resource as Input},
    shared::fine_config::FineConfig,
};

pub(super) fn descriptor(
    batch: &ComputeBatch,
    pass: &Pass,
    resources: &[Resource],
) -> Result<(objc2::rc::Retained<MTLRenderPassDescriptor>, FineConfig)> {
    let binding = |slot| {
        pass.bindings
            .iter()
            .find(|(b, _)| b.slot == slot)
            .map(|(_, id)| id.index())
            .ok_or("Metal fine binding missing")
    };
    let Input::Buffer(bytes) = &batch.resources()[binding(0)?] else {
        return Err("Metal fine config must be immutable host data".into());
    };
    let config: FineConfig =
        bytemuck::try_pod_read_unaligned(bytes).map_err(|_| "Metal fine config size mismatch")?;
    let target = resources[binding(1)?].texture()?;
    if !target.usage().contains(MTLTextureUsage::RenderTarget) {
        return Err("Metal fine target requires RenderTarget usage".into());
    }
    if config.width == 0
        || config.height == 0
        || config.width as usize > target.width()
        || config.height as usize > target.height()
    {
        return Err("Metal fine viewport exceeds target".into());
    }
    let descriptor = MTLRenderPassDescriptor::new();
    // SAFETY: attachment zero is always valid.
    let color = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
    color.setTexture(Some(target));
    color.setLoadAction(load_action(&config, [target.width(), target.height()]));
    color.setStoreAction(MTLStoreAction::Store);
    if config.incremental == 0 {
        descriptor.setTileWidth(16);
        descriptor.setTileHeight(16);
    }
    Ok((descriptor, config))
}

fn load_action(config: &FineConfig, size: [usize; 2]) -> MTLLoadAction {
    // Retained sparse updates and oversized host targets must preserve untouched
    // pixels. Full replacement can discard the attachment's previous contents.
    if config.load_target != 0
        || config.incremental != 0
        || size != [config.width as usize, config.height as usize]
    {
        MTLLoadAction::Load
    } else {
        MTLLoadAction::DontCare
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_load_preserves_sparse_blend_and_outside_viewport_pixels() {
        let mut config = FineConfig {
            width: 17,
            height: 15,
            ..Default::default()
        };
        assert_eq!(load_action(&config, [17, 15]), MTLLoadAction::DontCare);
        assert_eq!(load_action(&config, [32, 16]), MTLLoadAction::Load);
        config.incremental = 1;
        assert_eq!(load_action(&config, [17, 15]), MTLLoadAction::Load);
        config.incremental = 0;
        config.load_target = 1;
        assert_eq!(load_action(&config, [17, 15]), MTLLoadAction::Load);
    }
}
