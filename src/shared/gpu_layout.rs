#![allow(
    dead_code,
    reason = "Rust-side binding constants share one schema with generated WGSL variants"
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
    pub(crate) const LINEAR_SAMPLER_BINDING: u32 = 51;
    pub(crate) const IMAGE_RESOURCE_ATLAS_BINDING: u32 = 0;
    pub(crate) const IMAGE_RESOURCE_SAMPLER_BINDING: u32 = 1;
    pub(crate) const IMAGE_RESOURCE_TEXTURES_BINDING: u32 = 2;
}

#[cfg(test)]
mod tests {
    use super::{brush, filter, fine};

    const FINE_HEADER: &str = include_str!("../wgpu/shaders/fine/header.wgsl");
    const FILTER_HEADER: &str = include_str!("../wgpu/shaders/filter/header.wgsl");

    #[test]
    fn brush_shader_constants_match_rust_layout() {
        for source in [FINE_HEADER, FILTER_HEADER] {
            assert_wgsl_const(
                source,
                "GPU_BRUSH_U32_STRIDE",
                brush::GPU_BRUSH_U32_STRIDE as u32,
            );
            assert_wgsl_const(
                source,
                "GPU_BRUSH_PARAM_STRIDE",
                brush::GPU_BRUSH_PARAM_STRIDE as u32,
            );
            assert_wgsl_const(source, "GPU_BRUSH_LINEAR", brush::GPU_BRUSH_LINEAR);
            assert_wgsl_const(source, "GPU_BRUSH_RADIAL", brush::GPU_BRUSH_RADIAL);
            assert_wgsl_const(source, "GPU_BRUSH_SWEEP", brush::GPU_BRUSH_SWEEP);
            assert_wgsl_const(
                source,
                "GPU_BRUSH_FOUR_CORNER",
                brush::GPU_BRUSH_FOUR_CORNER,
            );
            assert_wgsl_const(source, "GPU_BRUSH_PATTERN", brush::GPU_BRUSH_PATTERN);
            assert_wgsl_const(
                source,
                "GPU_BRUSH_PATTERN_RESOURCE",
                brush::GPU_BRUSH_PATTERN_RESOURCE,
            );
            assert_wgsl_const(
                source,
                "GPU_RESOURCE_TEXTURE_PLACEMENT_BIT",
                brush::GPU_RESOURCE_TEXTURE_PLACEMENT_BIT,
            );
            assert_wgsl_const(
                source,
                "GPU_RESOURCE_TEXTURE_INDEX_MASK",
                brush::GPU_RESOURCE_TEXTURE_INDEX_MASK,
            );
            assert_wgsl_const(source, "GPU_PATTERN_BILINEAR", brush::GPU_PATTERN_BILINEAR);
            assert_wgsl_const(source, "GPU_EXTEND_REPEAT", brush::GPU_EXTEND_REPEAT);
            assert_wgsl_const(source, "GPU_EXTEND_REFLECT", brush::GPU_EXTEND_REFLECT);
        }
    }

    #[test]
    fn image_resource_shader_bindings_match_rust_layout() {
        assert_wgsl_texture_binding(
            FILTER_HEADER,
            filter::SOURCE_TEXTURE_BINDING,
            "source_texture",
            0,
            "texture_2d<f32>",
        );
        assert_wgsl_texture_binding(
            FILTER_HEADER,
            filter::AUX_TEXTURE_BINDING,
            "aux_texture",
            0,
            "texture_2d<f32>",
        );
        assert_wgsl_sampler_binding(
            FILTER_HEADER,
            filter::LINEAR_SAMPLER_BINDING,
            "filter_linear_sampler",
            0,
        );
        assert_wgsl_texture_binding(
            FINE_HEADER,
            fine::IMAGE_RESOURCE_ATLAS_BINDING,
            "image_resource_atlas",
            1,
            "texture_2d_array<f32>",
        );
        assert_wgsl_sampler_binding(
            FINE_HEADER,
            fine::IMAGE_RESOURCE_SAMPLER_BINDING,
            "image_resource_sampler",
            1,
        );
        assert_wgsl_texture_binding(
            FILTER_HEADER,
            filter::IMAGE_RESOURCE_ATLAS_BINDING,
            "image_resource_atlas",
            1,
            "texture_2d_array<f32>",
        );
        assert_wgsl_sampler_binding(
            FILTER_HEADER,
            filter::IMAGE_RESOURCE_SAMPLER_BINDING,
            "image_resource_sampler",
            1,
        );
    }

    fn assert_wgsl_const(source: &str, name: &str, expected: u32) {
        let needle = format!("const {name}: u32 = {expected}u;");
        assert!(source.contains(&needle), "missing WGSL constant `{needle}`");
    }

    fn assert_wgsl_texture_binding(source: &str, binding: u32, name: &str, group: u32, ty: &str) {
        let needle = format!("@group({group}) @binding({binding}) var {name}: {ty};");
        assert!(source.contains(&needle), "missing WGSL binding `{needle}`");
    }

    fn assert_wgsl_sampler_binding(source: &str, binding: u32, name: &str, group: u32) {
        let needle = format!("@group({group}) @binding({binding}) var {name}: sampler;");
        assert!(source.contains(&needle), "missing WGSL binding `{needle}`");
    }
}
