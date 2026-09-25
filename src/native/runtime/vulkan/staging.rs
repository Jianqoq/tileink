//! Completed-frame upload storage. In-flight frames retain exclusive ownership.
use super::{compute_memory::Arena, upload::Upload};
use ash::vk;

pub(super) struct Staging {
    pub arena: Arena,
    capacity: usize,
}

impl Staging {
    pub fn prepare(
        device: &ash::Device,
        memory: &vk::PhysicalDeviceMemoryProperties,
        upload: &Upload<'_>,
        cached: &mut Vec<Self>,
    ) -> super::Result<Self> {
        let reusable = cached
            .iter()
            .enumerate()
            .filter(|(_, staging)| staging.capacity >= upload.len())
            .min_by_key(|(_, staging)| staging.capacity)
            .map(|(index, _)| index);
        let storage = match reusable
            .map(|index| cached.swap_remove(index))
            .or_else(|| cached.pop())
        {
            Some(storage) if storage.capacity >= upload.len() => storage,
            _ => {
                let capacity = upload
                    .len()
                    .checked_next_power_of_two()
                    .ok_or("native Vulkan staging capacity overflow")?;
                Self {
                    arena: Arena::new(
                        device,
                        memory,
                        &[capacity as u64],
                        vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::UNIFORM_BUFFER,
                        vk::MemoryPropertyFlags::HOST_VISIBLE
                            | vk::MemoryPropertyFlags::HOST_COHERENT,
                    )?,
                    capacity,
                }
            }
        };
        storage.arena.write(upload)?;
        Ok(storage)
    }
}
