struct TestConfig { width: u32, height: u32 }
const config = TestConfig(192u, 128u);
@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var aux_texture: texture_2d<f32>;
@group(0) @binding(2) var target_texture: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8)
fn compare_sampling(@builtin(global_invocation_id) gid: vec3<u32>) {
    let index = gid.x + gid.y * 512u;
    // Cover exact texels, quarter/eighth phases and both sides of 1/256 ties,
    // as well as clamping at every logical edge, without dependence on capacity.
    let x = f32((index * 73u) % 194u) - 1.0 + f32((index * 199u) % 1024u) / 1024.0;
    let y = f32((index * 47u) % 130u) - 1.0 + f32((index * 173u) % 1024u) / 1024.0;
    let reference = filter_source_sample_premul(x, y);
    let actual = filter_aux_sample_premul(x, y);
    textureStore(target_texture, vec2<i32>(gid.xy), vec4<f32>(select(0.0, 1.0, any(reference != actual)), 0.0, 0.0, 1.0));
}

@compute @workgroup_size(8, 8)
fn sampling_contract(@builtin(global_invocation_id) gid: vec3<u32>) {
    let index = gid.x + gid.y * 512u;
    let positions = array<vec2<f32>, 10>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.5, 0.5), vec2<f32>(0.25, 0.75), vec2<f32>(-3.0, 0.5),
        vec2<f32>(3.0, 0.5), vec2<f32>(0.5, -3.0), vec2<f32>(0.5, 3.0),
    );
    let point = positions[index % 10u];
    var value: vec4<f32>;
    if ((index / 10u) % 2u == 0u) {
        value = filter_source_sample_premul(point.x, point.y);
    } else {
        value = filter_aux_sample_premul(point.x, point.y);
    }
    textureStore(target_texture, vec2<i32>(gid.xy), value);
}
