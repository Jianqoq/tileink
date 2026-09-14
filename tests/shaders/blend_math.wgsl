// Test adapter calls the full production blend helper, including every mode.
struct MathConfig { count:u32,pad0:u32,pad1:u32,pad2:u32 }
@group(0) @binding(0) var<uniform> config:MathConfig;
@group(0) @binding(1) var<storage,read> source:array<u32>;
@group(0) @binding(2) var<storage,read_write> destination:array<u32>;
@compute @workgroup_size(FINE_WORKGROUP_SIZE)
fn blend_math_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if (id.x>=config.count) { return; }
    let base=id.x*4u;
    destination[id.x]=blend_premul_u8(source[base+1u],source[base],source[base+2u]);
}
