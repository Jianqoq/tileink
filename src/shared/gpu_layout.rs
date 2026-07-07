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

    pub(crate) const GPU_PATTERN_NEAREST: u32 = 0;
    pub(crate) const GPU_PATTERN_BILINEAR: u32 = 1;

    pub(crate) const GPU_EXTEND_PAD: u32 = 0;
    pub(crate) const GPU_EXTEND_REPEAT: u32 = 1;
    pub(crate) const GPU_EXTEND_REFLECT: u32 = 2;
}

pub(crate) mod image_resource {}

pub(crate) mod fine {
    pub(crate) const STORAGE_BUFFER_COUNT: u32 = 6;
    pub(crate) const IMAGE_RESOURCE_ATLAS_BINDING: u32 = 8;
    pub(crate) const IMAGE_RESOURCE_SAMPLER_BINDING: u32 = 9;
}

pub(crate) mod filter {
    pub(crate) const MAX_STORAGE_BUFFER_COUNT: u32 = 8;
    pub(crate) const SOURCE_SAMPLE_TEXTURE_BINDING: u32 = 49;
    pub(crate) const AUX_SAMPLE_TEXTURE_BINDING: u32 = 50;
    pub(crate) const LINEAR_SAMPLER_BINDING: u32 = 51;
    pub(crate) const IMAGE_RESOURCE_ATLAS_BINDING: u32 = 56;
    pub(crate) const IMAGE_RESOURCE_SAMPLER_BINDING: u32 = 57;
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
            assert_wgsl_const(source, "GPU_PATTERN_BILINEAR", brush::GPU_PATTERN_BILINEAR);
            assert_wgsl_const(source, "GPU_EXTEND_REPEAT", brush::GPU_EXTEND_REPEAT);
            assert_wgsl_const(source, "GPU_EXTEND_REFLECT", brush::GPU_EXTEND_REFLECT);
        }
    }

    #[test]
    fn image_resource_shader_bindings_match_rust_layout() {
        assert_wgsl_texture_binding(
            FILTER_HEADER,
            filter::SOURCE_SAMPLE_TEXTURE_BINDING,
            "filter_source_sample_texture",
        );
        assert_wgsl_texture_binding(
            FILTER_HEADER,
            filter::AUX_SAMPLE_TEXTURE_BINDING,
            "filter_aux_sample_texture",
        );
        assert_wgsl_sampler_binding(
            FILTER_HEADER,
            filter::LINEAR_SAMPLER_BINDING,
            "filter_linear_sampler",
        );
        assert_wgsl_texture_binding(
            FINE_HEADER,
            fine::IMAGE_RESOURCE_ATLAS_BINDING,
            "image_resource_atlas",
        );
        assert_wgsl_sampler_binding(
            FINE_HEADER,
            fine::IMAGE_RESOURCE_SAMPLER_BINDING,
            "image_resource_sampler",
        );
        assert_wgsl_texture_binding(
            FILTER_HEADER,
            filter::IMAGE_RESOURCE_ATLAS_BINDING,
            "image_resource_atlas",
        );
        assert_wgsl_sampler_binding(
            FILTER_HEADER,
            filter::IMAGE_RESOURCE_SAMPLER_BINDING,
            "image_resource_sampler",
        );
    }

    fn assert_wgsl_const(source: &str, name: &str, expected: u32) {
        let needle = format!("const {name}: u32 = {expected}u;");
        assert!(source.contains(&needle), "missing WGSL constant `{needle}`");
    }

    fn assert_wgsl_texture_binding(source: &str, binding: u32, name: &str) {
        let needle = format!("@group(0) @binding({binding}) var {name}: texture_2d<f32>;");
        assert!(source.contains(&needle), "missing WGSL binding `{needle}`");
    }

    fn assert_wgsl_sampler_binding(source: &str, binding: u32, name: &str) {
        let needle = format!("@group(0) @binding({binding}) var {name}: sampler;");
        assert!(source.contains(&needle), "missing WGSL binding `{needle}`");
    }
}
