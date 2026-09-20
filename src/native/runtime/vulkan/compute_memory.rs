//! A submission owns three memory blocks, not one allocation per logical buffer.
use super::super::Result;
use ash::vk;

pub struct Arena {
    device: ash::Device,
    pub buffers: Vec<vk::Buffer>,
    memory: vk::DeviceMemory,
}
impl Arena {
    pub fn new(
        device: &ash::Device,
        properties: &vk::PhysicalDeviceMemoryProperties,
        sizes: &[u64],
        usage: vk::BufferUsageFlags,
        flags: vk::MemoryPropertyFlags,
    ) -> Result<Self> {
        let mut this = Self {
            device: device.clone(),
            buffers: Vec::new(),
            memory: vk::DeviceMemory::null(),
        };
        if sizes.is_empty() {
            return Ok(this);
        }
        unsafe {
            let mut offsets = Vec::new();
            let mut total = 0u64;
            let mut bits = u32::MAX;
            for &size in sizes {
                let buffer = device.create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(size)
                        .usage(usage)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE),
                    None,
                )?;
                this.buffers.push(buffer);
                let requirements = device.get_buffer_memory_requirements(buffer);
                bits &= requirements.memory_type_bits;
                total = align(total, requirements.alignment)?;
                offsets.push(total);
                total = total
                    .checked_add(requirements.size)
                    .ok_or("native Vulkan arena size overflow")?;
            }
            let index = (0..properties.memory_type_count)
                .find(|i| {
                    bits & (1 << i) != 0
                        && properties.memory_types[*i as usize]
                            .property_flags
                            .contains(flags)
                })
                .ok_or("native Vulkan memory type unavailable")?;
            this.memory = device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(total)
                    .memory_type_index(index),
                None,
            )?;
            for (&buffer, &offset) in this.buffers.iter().zip(&offsets) {
                device.bind_buffer_memory(buffer, this.memory, offset)?;
            }
        }
        Ok(this)
    }
    /// Staging arenas contain exactly one host-coherent buffer at memory offset zero.
    pub(super) fn write(&self, upload: &super::upload::Upload<'_>) -> Result<()> {
        unsafe {
            let pointer = self.device.map_memory(
                self.memory,
                0,
                upload.len() as u64,
                vk::MemoryMapFlags::empty(),
            )?;
            upload.copy_to(pointer.cast());
            self.device.unmap_memory(self.memory);
        }
        Ok(())
    }
    pub fn read(&self, size: usize) -> Result<Vec<u8>> {
        unsafe {
            let p =
                self.device
                    .map_memory(self.memory, 0, size as u64, vk::MemoryMapFlags::empty())?;
            let bytes = std::slice::from_raw_parts(p.cast::<u8>(), size).to_vec();
            self.device.unmap_memory(self.memory);
            Ok(bytes)
        }
    }
}
impl Drop for Arena {
    fn drop(&mut self) {
        unsafe {
            for buffer in self.buffers.drain(..) {
                self.device.destroy_buffer(buffer, None);
            }
            self.device.free_memory(self.memory, None);
        }
    }
}
pub fn align(value: u64, alignment: u64) -> Result<u64> {
    let mask = alignment
        .checked_sub(1)
        .filter(|_| alignment.is_power_of_two())
        .ok_or("invalid native buffer alignment")?;
    Ok(value
        .checked_add(mask)
        .ok_or("native buffer alignment overflow")?
        & !mask)
}
#[test]
fn arena_alignment_checks_overflow() {
    assert_eq!(align(257, 256).unwrap(), 512);
    assert!(align(u64::MAX, 256).is_err());
    assert!(align(1, 0).is_err());
    assert!(align(1, 3).is_err());
}
