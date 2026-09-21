use super::{compute_memory::Arena, staging::Staging, *};

mod commands;
pub(super) use commands::Commands;

#[derive(Default)]
pub(super) struct Cache {
    pub(super) samplers: super::compute_sampler::Cache,
    pub staging: Option<Staging>,
    storage: Vec<Arena>,
    pub(super) commands: Vec<Commands>,
}

impl Cache {
    pub(super) fn acquire_commands(
        &mut self,
        device: &ash::Device,
        family: u32,
        passes: &[super::super::compute::Pass],
    ) -> Result<Commands> {
        // Empty batches can reuse command storage without hiding a descriptor pool
        // needed by the next draw. All candidates have completed their fence wait.
        let preferred = self
            .commands
            .iter()
            .rposition(|commands| {
                (commands.descriptors == vk::DescriptorPool::null()) == passes.is_empty()
            })
            .or_else(|| self.commands.len().checked_sub(1));
        let cached = preferred.map(|index| self.commands.swap_remove(index));
        Commands::prepare(device, family, passes, cached)
    }

    pub(super) fn acquire_storage(
        &mut self,
        device: &ash::Device,
        memory: &vk::PhysicalDeviceMemoryProperties,
        sizes: &[u64],
    ) -> Result<Arena> {
        // An empty batch must not consume and discard a useful completed arena.
        let cached = if sizes.is_empty() {
            None
        } else {
            self.storage.pop()
        };
        if let Some(arena) = &cached
            && arena.capacities.len() >= sizes.len()
            && arena
                .capacities
                .iter()
                .zip(sizes)
                .all(|(capacity, size)| capacity >= size)
        {
            return Ok(cached.unwrap());
        }
        let capacities = sizes
            .iter()
            .enumerate()
            .map(|(index, size)| {
                let required = size
                    .checked_next_power_of_two()
                    .ok_or("Vulkan storage capacity overflow")?;
                Ok(cached
                    .as_ref()
                    .and_then(|arena| arena.capacities.get(index))
                    .copied()
                    .unwrap_or(0)
                    .max(required))
            })
            .collect::<Result<Vec<_>>>()?;
        Arena::new(
            device,
            memory,
            &capacities,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::UNIFORM_BUFFER
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::TRANSFER_DST,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )
    }

    /// Called only after a successful fence wait. Unknown or rejected submissions
    /// never return their owners here. Retain every retired slot for pipeline refill.
    pub(super) fn retire_storage(&mut self, arena: Arena) {
        if !arena.buffers.is_empty() {
            self.storage.push(arena);
        }
    }
}
