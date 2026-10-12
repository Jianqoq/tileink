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
        // SAFETY: The sampler descriptor uses supported fixed modes on this live device; the returned sampler is retained by its owner.
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
            device: std::rc::Rc::clone(device),
            handle,
        })
    }
}
impl Drop for Sampler {
    fn drop(&mut self) {
        // SAFETY: Pending frames retain this immutable sampler owner until completion; this destructor releases its exclusively owned handle once.
        unsafe {
            self.device.destroy_sampler(self.handle, None);
        }
    }
}
// Sampler state is immutable, so concurrent frames can share one owner per
// filtering mode. This removes per-frame vkCreateSampler/vkDestroySampler churn.
#[derive(Default)]
pub(super) struct Cache {
    nearest: Option<std::rc::Rc<Sampler>>,
    linear: Option<std::rc::Rc<Sampler>>,
}

impl Cache {
    pub(super) fn get(
        &mut self,
        device: &ash::Device,
        filter: SamplerFilter,
    ) -> Result<std::rc::Rc<Sampler>> {
        let slot = match filter {
            SamplerFilter::Nearest => &mut self.nearest,
            SamplerFilter::Linear => &mut self.linear,
        };
        let sampler = if let Some(sampler) = slot {
            sampler
        } else {
            slot.insert(std::rc::Rc::new(Sampler::new(
                &std::rc::Rc::new(device.clone()),
                filter,
            )?))
        };
        Ok(std::rc::Rc::clone(sampler))
    }
}
