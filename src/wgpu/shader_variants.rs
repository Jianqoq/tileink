pub(crate) fn patch_image_resource_shader_source(
    source: &str,
    large_texture_table_enabled: bool,
) -> String {
    let binding = if large_texture_table_enabled {
        "@group(1) @binding(2) var image_resource_textures: binding_array<texture_2d<f32>>;"
    } else {
        ""
    };
    let functions = if large_texture_table_enabled {
        LARGE_TEXTURE_TABLE_FUNCTIONS
    } else {
        LARGE_TEXTURE_TABLE_DISABLED_FUNCTIONS
    };
    let patched = source
        .replace("// TILEINK_IMAGE_RESOURCE_TEXTURE_TABLE_BINDING", binding)
        .replace(
            "// TILEINK_IMAGE_RESOURCE_TEXTURE_TABLE_FUNCTIONS",
            functions,
        );
    if large_texture_table_enabled {
        format!("enable wgpu_binding_array;\n{patched}")
    } else {
        patched
    }
}

const LARGE_TEXTURE_TABLE_DISABLED_FUNCTIONS: &str = r#"
fn sample_resource_pattern_texture(
    tx: f32,
    ty: f32,
    texture_index: u32,
    width: u32,
    height: u32,
    opacity: u32,
    extend: u32,
    sampling: u32,
) -> u32 {
    return 0u;
}
"#;

const LARGE_TEXTURE_TABLE_FUNCTIONS: &str = r#"
fn sample_resource_pattern_texture(
    tx: f32,
    ty: f32,
    texture_index: u32,
    width: u32,
    height: u32,
    opacity: u32,
    extend: u32,
    sampling: u32,
) -> u32 {
    var color = 0u;
    if (sampling == GPU_PATTERN_BILINEAR && extend == 0u) {
        let dims = vec2<f32>(textureDimensions(image_resource_textures[texture_index]));
        let local = vec2<f32>(
            clamp(tx, 0.0, f32(width)),
            clamp(ty, 0.0, f32(height)),
        );
        let uv = local / dims;
        color = unorm_to_rgba8(textureSampleLevel(image_resource_textures[texture_index], image_resource_sampler, uv, 0.0));
    } else if (sampling == GPU_PATTERN_BILINEAR) {
        let sx = tx - 0.5;
        let sy = ty - 0.5;
        let x0f = floor(sx);
        let y0f = floor(sy);
        let fx = sx - x0f;
        let fy = sy - y0f;
        let x0 = i32(x0f);
        let y0 = i32(y0f);
        let tl = texture_pattern_pixel(texture_index, width, height, extend, x0, y0);
        let tr = texture_pattern_pixel(texture_index, width, height, extend, x0 + 1, y0);
        let bl = texture_pattern_pixel(texture_index, width, height, extend, x0, y0 + 1);
        let br = texture_pattern_pixel(texture_index, width, height, extend, x0 + 1, y0 + 1);
        color = lerp_premul_u8(lerp_premul_u8(tl, tr, fx), lerp_premul_u8(bl, br, fx), fy);
    } else {
        color = texture_pattern_pixel(texture_index, width, height, extend, i32(floor(tx)), i32(floor(ty)));
    }
    return scale_premul_u8(color, opacity);
}

fn texture_pattern_pixel(texture_index: u32, width: u32, height: u32, extend: u32, x: i32, y: i32) -> u32 {
    let local_x = extend_coord_i32(x, width, extend);
    let local_y = extend_coord_i32(y, height, extend);
    return unorm_to_rgba8(textureLoad(image_resource_textures[texture_index], vec2<i32>(i32(local_x), i32(local_y)), 0));
}
"#;

#[cfg(test)]
mod tests {
    use super::patch_image_resource_shader_source;

    #[test]
    fn image_resource_variant_replaces_every_marker() {
        let source = "// TILEINK_IMAGE_RESOURCE_TEXTURE_TABLE_BINDING\n// TILEINK_IMAGE_RESOURCE_TEXTURE_TABLE_FUNCTIONS";
        for enabled in [false, true] {
            let patched = patch_image_resource_shader_source(source, enabled);
            assert!(!patched.contains("TILEINK_IMAGE_RESOURCE_TEXTURE_TABLE"));
            assert_eq!(patched.starts_with("enable wgpu_binding_array;"), enabled);
        }
    }
}
