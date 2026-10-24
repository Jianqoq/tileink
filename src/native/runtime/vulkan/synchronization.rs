use super::*;
use crate::native::interop::vulkan::{SemaphorePoint, TargetSynchronization};
pub fn validate(
    sync: &TargetSynchronization,
    family: u32,
    families: &[vk::QueueFamilyProperties],
    timeline: bool,
) -> Result<()> {
    let layout = |layout| {
        matches!(
            layout,
            vk::ImageLayout::GENERAL
                | vk::ImageLayout::TRANSFER_SRC_OPTIMAL
                | vk::ImageLayout::TRANSFER_DST_OPTIMAL
                | vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
                | vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL
                | vk::ImageLayout::PRESENT_SRC_KHR
        )
    };
    if !(layout(sync.incoming.layout) || sync.incoming.layout == vk::ImageLayout::UNDEFINED)
        || !layout(sync.outgoing.layout)
    {
        return Err("unsupported Vulkan target-use layout".into());
    }
    for state in [sync.incoming, sync.outgoing] {
        if state.stages.is_empty()
            || (state.queue_family != vk::QUEUE_FAMILY_IGNORED
                && families
                    .get(state.queue_family as usize)
                    .is_none_or(|properties| properties.queue_count == 0))
        {
            return Err("invalid Vulkan target scope or queue family".into());
        }
        let scope_family = if state.queue_family == vk::QUEUE_FAMILY_IGNORED {
            family
        } else {
            state.queue_family
        };
        validate_scope(
            state.stages,
            state.access,
            families[scope_family as usize].queue_flags,
        )?;
        if state.layout == vk::ImageLayout::UNDEFINED && !state.access.is_empty() {
            return Err("undefined Vulkan image contents have no source accesses".into());
        }
    }
    if sync.incoming.queue_family != vk::QUEUE_FAMILY_IGNORED
        && sync.incoming.queue_family != family
        && sync.waits.is_empty()
    {
        return Err("cross-family acquire requires an explicit semaphore dependency".into());
    }
    for point in sync
        .waits
        .iter()
        .map(|wait| wait.semaphore)
        .chain(sync.signals.iter().copied())
    {
        if point.handle() == vk::Semaphore::null()
            || (matches!(point, SemaphorePoint::Timeline { .. }) && !timeline)
        {
            return Err("null semaphore or timelineSemaphore was not enabled".into());
        }
    }
    for (index, wait) in sync.waits.iter().enumerate() {
        validate_scope(
            wait.stages,
            vk::AccessFlags::empty(),
            families[family as usize].queue_flags,
        )?;
        if wait.stages.is_empty()
            || wait.stages.contains(vk::PipelineStageFlags::HOST)
            || sync.waits[..index]
                .iter()
                .any(|old| old.semaphore.handle() == wait.semaphore.handle())
        {
            return Err("invalid or duplicate Vulkan semaphore wait".into());
        }
    }
    for (index, signal) in sync.signals.iter().enumerate() {
        if sync.signals[..index]
            .iter()
            .any(|old| old.handle() == signal.handle())
        {
            return Err("duplicate Vulkan semaphore signal".into());
        }
        if let Some(wait) = sync
            .waits
            .iter()
            .find(|wait| wait.semaphore.handle() == signal.handle())
            && !matches!((wait.semaphore, signal), (SemaphorePoint::Timeline { value: before, .. }, SemaphorePoint::Timeline { value: after, .. }) if *after > before)
        {
            return Err("a repeated semaphore requires increasing timeline values".into());
        }
    }
    Ok(())
}

fn validate_scope(
    stages: vk::PipelineStageFlags,
    access: vk::AccessFlags,
    queue: vk::QueueFlags,
) -> Result<()> {
    let graphics = vk::PipelineStageFlags::DRAW_INDIRECT
        | vk::PipelineStageFlags::VERTEX_INPUT
        | vk::PipelineStageFlags::VERTEX_SHADER
        | vk::PipelineStageFlags::FRAGMENT_SHADER
        | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
        | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS
        | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
        | vk::PipelineStageFlags::ALL_GRAPHICS;
    let common = vk::PipelineStageFlags::TOP_OF_PIPE
        | vk::PipelineStageFlags::BOTTOM_OF_PIPE
        | vk::PipelineStageFlags::TRANSFER
        | vk::PipelineStageFlags::HOST
        | vk::PipelineStageFlags::ALL_COMMANDS;
    let supported = common
        | if queue.contains(vk::QueueFlags::GRAPHICS) {
            graphics
        } else {
            vk::PipelineStageFlags::empty()
        }
        | if queue.contains(vk::QueueFlags::COMPUTE) {
            vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::DRAW_INDIRECT
        } else {
            vk::PipelineStageFlags::empty()
        };
    if stages.is_empty() || stages.as_raw() & !supported.as_raw() != 0 {
        return Err("Vulkan stage is unsupported by the target queue".into());
    }
    let all = stages.contains(vk::PipelineStageFlags::ALL_COMMANDS);
    let mut supported_access = vk::AccessFlags::empty();
    if all || stages.intersects(vk::PipelineStageFlags::TRANSFER) {
        supported_access |= vk::AccessFlags::TRANSFER_READ | vk::AccessFlags::TRANSFER_WRITE;
    }
    let shader = vk::PipelineStageFlags::COMPUTE_SHADER
        | vk::PipelineStageFlags::VERTEX_SHADER
        | vk::PipelineStageFlags::FRAGMENT_SHADER
        | vk::PipelineStageFlags::ALL_GRAPHICS;
    if (all && queue.intersects(vk::QueueFlags::COMPUTE | vk::QueueFlags::GRAPHICS))
        || stages.intersects(shader)
    {
        supported_access |= vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE;
    }
    if queue.contains(vk::QueueFlags::GRAPHICS) {
        if all
            || stages.intersects(
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                    | vk::PipelineStageFlags::ALL_GRAPHICS,
            )
        {
            supported_access |=
                vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE;
        }
        if all
            || stages.intersects(
                vk::PipelineStageFlags::FRAGMENT_SHADER | vk::PipelineStageFlags::ALL_GRAPHICS,
            )
        {
            supported_access |= vk::AccessFlags::INPUT_ATTACHMENT_READ;
        }
    }
    if stages.contains(vk::PipelineStageFlags::HOST) {
        supported_access |= vk::AccessFlags::HOST_READ | vk::AccessFlags::HOST_WRITE;
    }
    if stages.as_raw()
        & !(vk::PipelineStageFlags::TOP_OF_PIPE | vk::PipelineStageFlags::BOTTOM_OF_PIPE).as_raw()
        != 0
    {
        supported_access |= vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE;
    }
    if access.as_raw() & !supported_access.as_raw() != 0 {
        return Err("Vulkan image access is incompatible with its stage scope".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsupported_stages_and_incompatible_image_accesses() {
        let queue = vk::QueueFlags::COMPUTE;
        assert!(
            validate_scope(
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::AccessFlags::SHADER_READ,
                queue
            )
            .is_err()
        );
        assert!(
            validate_scope(
                vk::PipelineStageFlags::TRANSFER,
                vk::AccessFlags::SHADER_WRITE,
                queue
            )
            .is_err()
        );
        assert!(
            validate_scope(
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::AccessFlags::MEMORY_READ,
                queue
            )
            .is_err()
        );
        assert!(
            validate_scope(
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
                queue
            )
            .is_ok()
        );
        assert!(
            validate_scope(
                vk::PipelineStageFlags::TRANSFER,
                vk::AccessFlags::TRANSFER_WRITE,
                queue
            )
            .is_ok()
        );
    }
}
