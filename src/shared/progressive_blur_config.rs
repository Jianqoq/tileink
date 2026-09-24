/// Dedicated ABI: changing progressive blur does not enlarge every filter uniform.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProgressiveBlurConfig {
    pub output: [u32; 4],
    pub source: [u32; 4],
    pub gradient: [f32; 4],
    pub max_std_dev: f32,
    pub level_count: u32,
    pub step: u32,
    pub pad: u32,
}
