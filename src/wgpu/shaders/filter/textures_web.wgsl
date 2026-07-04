@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var aux_texture: texture_2d<f32>;
@group(0) @binding(3) var target_texture: texture_storage_2d<rgba8unorm, write>;

fn filter_source_load(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(textureLoad(source_texture, vec2<i32>(i32(x), i32(y)), 0));
}

fn filter_aux_load(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(textureLoad(aux_texture, vec2<i32>(i32(x), i32(y)), 0));
}

fn filter_target_load(x: u32, y: u32) -> u32 {
    return filter_source_load(x, y);
}

fn filter_target_store(x: u32, y: u32, pixel: u32) {
    textureStore(target_texture, vec2<i32>(i32(x), i32(y)), rgba8_to_unorm(pixel));
}
