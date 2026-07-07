@group(0) @binding(1) var source_texture: texture_storage_2d<rgba8unorm, read>;
@group(0) @binding(2) var aux_texture: texture_storage_2d<rgba8unorm, read>;
@group(0) @binding(3) var target_texture: texture_storage_2d<rgba8unorm, read_write>;

fn filter_source_load(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(textureLoad(source_texture, vec2<i32>(i32(x), i32(y))));
}

fn filter_aux_load(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(textureLoad(aux_texture, vec2<i32>(i32(x), i32(y))));
}

fn filter_source_sample_premul(x: f32, y: f32) -> vec4<f32> {
    let uv = (vec2<f32>(x, y) + vec2<f32>(0.5)) / vec2<f32>(f32(config.width), f32(config.height));
    return textureSampleLevel(filter_source_sample_texture, filter_linear_sampler, uv, 0.0);
}

fn filter_aux_sample_premul(x: f32, y: f32) -> vec4<f32> {
    let uv = (vec2<f32>(x, y) + vec2<f32>(0.5)) / vec2<f32>(f32(config.width), f32(config.height));
    return textureSampleLevel(filter_aux_sample_texture, filter_linear_sampler, uv, 0.0);
}

fn filter_target_load(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(textureLoad(target_texture, vec2<i32>(i32(x), i32(y))));
}

fn filter_target_store(x: u32, y: u32, pixel: u32) {
    textureStore(target_texture, vec2<i32>(i32(x), i32(y)), rgba8_to_unorm(pixel));
}
