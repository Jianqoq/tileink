@group(0) @binding(3) var target_texture: texture_storage_2d<rgba8unorm, read_write>;

fn filter_target_load(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(textureLoad(target_texture, vec2<i32>(i32(x), i32(y))));
}
