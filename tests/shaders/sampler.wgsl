struct SamplerConfig { count:u32, pad0:u32, pad1:u32, pad2:u32 }
@group(0) @binding(0) var<uniform> config:SamplerConfig;
@group(0) @binding(1) var source:texture_2d_array<f32>;
@group(0) @binding(2) var image_sampler:sampler;
@group(0) @binding(3) var<storage,read> requests:array<vec4<u32>>;
@group(0) @binding(4) var<storage,read_write> destination:array<u32>;
@compute @workgroup_size(FINE_WORKGROUP_SIZE,1,1)
fn sampler_words(@builtin(global_invocation_id) id:vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>) {
    let index=id.x+id.y*groups.x*FINE_WORKGROUP_SIZE;
    if (index >= config.count) { return; }
    let q=requests[index];
    let pixel=textureSampleLevel(source,image_sampler,bitcast<vec2<f32>>(q.xy),i32(q.z),0.0);
    destination[index]=unorm_to_rgba8(pixel);
}
