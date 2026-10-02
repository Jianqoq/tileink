#![allow(
    dead_code,
    reason = "Rust-side binding constants share one schema with native shaders"
)]

pub(crate) mod brush {
    pub(crate) const GPU_BRUSH_U32_STRIDE: usize = 9;
    pub(crate) const GPU_BRUSH_PARAM_STRIDE: usize = 12;
    pub(crate) const GPU_BRUSH_SOLID: u32 = 1;
    pub(crate) const GPU_BRUSH_LINEAR: u32 = 2;
    pub(crate) const GPU_BRUSH_RADIAL: u32 = 3;
    pub(crate) const GPU_BRUSH_SWEEP: u32 = 4;
    pub(crate) const GPU_BRUSH_FOUR_CORNER: u32 = 5;
    pub(crate) const GPU_BRUSH_PATTERN: u32 = 6;
    pub(crate) const GPU_BRUSH_PATTERN_RESOURCE: u32 = 7;
    pub(crate) const GPU_RESOURCE_TEXTURE_PLACEMENT_BIT: u32 = 0x8000_0000;
    pub(crate) const GPU_RESOURCE_TEXTURE_INDEX_MASK: u32 = 0x7fff_ffff;

    pub(crate) const GPU_PATTERN_NEAREST: u32 = 0;
    pub(crate) const GPU_PATTERN_BILINEAR: u32 = 1;

    pub(crate) const GPU_EXTEND_PAD: u32 = 0;
    pub(crate) const GPU_EXTEND_REPEAT: u32 = 1;
    pub(crate) const GPU_EXTEND_REFLECT: u32 = 2;
}

pub(crate) mod fine {
    pub(crate) const STORAGE_BUFFER_COUNT: u32 = 6;
    pub(crate) const IMAGE_RESOURCE_ATLAS_BINDING: u32 = 0;
    pub(crate) const IMAGE_RESOURCE_SAMPLER_BINDING: u32 = 1;
    pub(crate) const IMAGE_RESOURCE_TEXTURES_BINDING: u32 = 2;
}

pub(crate) mod filter {
    // The retained filter path adds the compact active-tile worklist to the
    // largest scene-stack kernel (seven scene buffers plus this worklist).
    pub(crate) const MAX_STORAGE_BUFFER_COUNT: u32 = 8;
    pub(crate) const SOURCE_TEXTURE_BINDING: u32 = 1;
    pub(crate) const AUX_TEXTURE_BINDING: u32 = 2;
    pub(crate) const IMAGE_RESOURCE_ATLAS_BINDING: u32 = 0;
    pub(crate) const IMAGE_RESOURCE_SAMPLER_BINDING: u32 = 1;
    pub(crate) const IMAGE_RESOURCE_TEXTURES_BINDING: u32 = 2;
}

#[cfg(test)]
mod tests {
    use super::brush;

    #[test]
    fn native_brush_constants_match_serialized_layout() {
        let source = include_str!("../shaders/hlsl/shared/brush/constants.hlsli");
        let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
        for (name, expected) in [
            ("BRUSH_HEADER_WORDS", brush::GPU_BRUSH_U32_STRIDE as u32),
            ("BRUSH_PARAM_WORDS", brush::GPU_BRUSH_PARAM_STRIDE as u32),
            ("BRUSH_SOLID", brush::GPU_BRUSH_SOLID),
            ("BRUSH_LINEAR", brush::GPU_BRUSH_LINEAR),
            ("BRUSH_RADIAL", brush::GPU_BRUSH_RADIAL),
            ("BRUSH_SWEEP", brush::GPU_BRUSH_SWEEP),
            ("BRUSH_FOUR_CORNER", brush::GPU_BRUSH_FOUR_CORNER),
            ("BRUSH_PATTERN", brush::GPU_BRUSH_PATTERN),
            ("BRUSH_PATTERN_RESOURCE", brush::GPU_BRUSH_PATTERN_RESOURCE),
            ("BRUSH_EXTEND_REPEAT", brush::GPU_EXTEND_REPEAT),
            ("BRUSH_EXTEND_REFLECT", brush::GPU_EXTEND_REFLECT),
        ] {
            let declaration = format!("staticconstuint{name}={expected}u;");

            assert!(compact.contains(&declaration), "HLSL brush layout {name}");
        }
    }
}
