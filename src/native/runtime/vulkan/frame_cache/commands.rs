use super::super::*;
use crate::native::{runtime::compute::Pass, shaders::BindingKind};

/// Moves from a frame into its context cache only after a successful fence wait.
/// Pool reset and fence reset must never race an in-flight command buffer.
pub(crate) struct Commands {
    device: ash::Device,
    pool: vk::CommandPool,
    pub descriptors: vk::DescriptorPool,
    pub fence: vk::Fence,
    pub command: vk::CommandBuffer,
    counts: [u32; 5],
    sets: u32,
}

impl Commands {
    pub(super) fn prepare(
        device: &ash::Device,
        family: u32,
        passes: &[Pass],
        cached: Option<Self>,
    ) -> Result<Self> {
        let mut this = if let Some(cached) = cached {
            unsafe {
                device.reset_command_pool(cached.pool, vk::CommandPoolResetFlags::empty())?;
                device.reset_fences(&[cached.fence])?;
            }
            cached
        } else {
            Self::new(device, family)?
        };
        this.prepare_descriptors(passes)?;
        Ok(this)
    }

    fn new(device: &ash::Device, family: u32) -> Result<Self> {
        let mut this = Self {
            device: device.clone(),
            pool: vk::CommandPool::null(),
            descriptors: vk::DescriptorPool::null(),
            fence: vk::Fence::null(),
            command: vk::CommandBuffer::null(),
            counts: [0; 5],
            sets: 0,
        };
        unsafe {
            this.pool = device.create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(family),
                None,
            )?;
            this.fence = device.create_fence(&vk::FenceCreateInfo::default(), None)?;
            this.command = device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(this.pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?[0];
        }
        Ok(this)
    }

    fn prepare_descriptors(&mut self, passes: &[Pass]) -> Result<()> {
        let mut counts = [0u32; 5];
        for pass in passes {
            for binding in pass.shader.bindings {
                let index = match binding.kind {
                    BindingKind::Uniform => 0,
                    BindingKind::Read | BindingKind::Write => 1,
                    BindingKind::Texture
                    | BindingKind::TextureArray
                    | BindingKind::TextureTable => 2,
                    BindingKind::TextureWrite => 3,
                    BindingKind::Sampler => 4,
                };
                counts[index] = counts[index]
                    .checked_add(binding.count)
                    .ok_or("native Vulkan descriptor count overflow")?;
            }
        }
        let sets = u32::try_from(passes.len())?;
        if self.descriptors != vk::DescriptorPool::null()
            && sets <= self.sets
            && counts
                .iter()
                .zip(self.counts)
                .all(|(needed, capacity)| *needed <= capacity)
        {
            unsafe {
                self.device.reset_descriptor_pool(
                    self.descriptors,
                    vk::DescriptorPoolResetFlags::empty(),
                )?;
            }
            return Ok(());
        }
        if sets == 0 {
            return Ok(());
        }
        let sets = retained_capacity(self.sets, sets)?;
        for (count, previous) in counts.iter_mut().zip(self.counts) {
            *count = retained_capacity(previous, *count)?;
        }
        let sizes: Vec<_> = [
            vk::DescriptorType::UNIFORM_BUFFER,
            vk::DescriptorType::STORAGE_BUFFER,
            vk::DescriptorType::SAMPLED_IMAGE,
            vk::DescriptorType::STORAGE_IMAGE,
            vk::DescriptorType::SAMPLER,
        ]
        .into_iter()
        .zip(counts)
        .filter(|(_, count)| *count != 0)
        .map(|(ty, descriptor_count)| vk::DescriptorPoolSize {
            ty,
            descriptor_count,
        })
        .collect();
        unsafe {
            let pool = self.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(sets)
                    .pool_sizes(&sizes),
                None,
            )?;
            self.device.destroy_descriptor_pool(self.descriptors, None);
            self.descriptors = pool;
        }
        self.counts = counts;
        self.sets = sets;
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::native::runtime::vulkan) fn handles(
        &self,
    ) -> (vk::CommandPool, vk::DescriptorPool, vk::Fence) {
        (self.pool, self.descriptors, self.fence)
    }
}

impl Drop for Commands {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_descriptor_pool(self.descriptors, None);
            self.device.destroy_command_pool(self.pool, None);
            self.device.destroy_fence(self.fence, None);
        }
    }
}

// Capacity growth must preserve every descriptor class when demand alternates.
fn retained_capacity(previous: u32, needed: u32) -> Result<u32> {
    if needed <= previous {
        return Ok(previous);
    }
    needed
        .checked_next_power_of_two()
        .ok_or_else(|| "native Vulkan descriptor capacity overflow".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternating_descriptor_demand_preserves_high_water_capacity() -> Result<()> {
        let mut capacities = [0; 3];
        // Set count, storage buffers, sampled images change independently.
        for needed in [[40, 80, 0], [1, 1, 20], [40, 80, 0], [1, 1, 20]] {
            for (capacity, needed) in capacities.iter_mut().zip(needed) {
                *capacity = retained_capacity(*capacity, needed)?;
            }
        }
        assert_eq!(capacities, [64, 128, 32]);
        assert!(retained_capacity(0, u32::MAX).is_err());
        Ok(())
    }
}
