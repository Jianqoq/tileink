use super::super::Result;
use super::super::compute::SamplerFilter;
use ash::vk;

pub(super) struct Sampler {
    device: std::rc::Rc<ash::Device>,
    pub handle: vk::Sampler,
}
impl Sampler {
    pub fn new(device: &std::rc::Rc<ash::Device>, filter: SamplerFilter) -> Result<Self> {
        let filter = match filter {
            SamplerFilter::Nearest => vk::Filter::NEAREST,
            SamplerFilter::Linear => vk::Filter::LINEAR,
        };
        let handle = unsafe {
            device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .min_filter(filter)
                    .mag_filter(filter)
                    .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .min_lod(0.0)
                    .max_lod(0.0),
                None,
            )?
        };
        Ok(Self {
            device: device.clone(),
            handle,
        })
    }
}
impl Drop for Sampler {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_sampler(self.handle, None);
        }
    }
}
