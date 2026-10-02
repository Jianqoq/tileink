struct Params {
    count:u32,
    source_offset:u32,
    destination_offset:u32,
    stride:u32,
    value:vec4<u32>,
}
@group(0) @binding(0) var<storage,read_write> destination:array<u32>;
@group(0) @binding(1) var<storage,read> source:array<u32>;
@group(0) @binding(2) var<uniform> params:Params;
@group(0) @binding(3) var texels:texture_2d<f32>;

@compute @workgroup_size(64)
fn clear_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x<params.count {destination[params.destination_offset/4+id.x]=params.value.x;}
}
@compute @workgroup_size(64)
fn copy_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x<params.count {destination[params.destination_offset/4+id.x]=source[params.source_offset/4+id.x];}
}
@compute @workgroup_size(64)
fn layout_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x<params.count {
        let values=params.value+vec4<u32>(id.x,params.source_offset,params.stride,params.count);
        let offset=(params.destination_offset+id.x*params.stride)/4;
        for(var lane=0u;lane<4u;lane++) {destination[offset+lane]=values[lane];}
    }
}

fn coordinate(bits:u32) -> i32 {
    let exponent = (bits >> 23u) & 255u;
    if exponent <= 109u { return 0; }
    let significand = (bits & 0x7fffffu) | 0x800000u;
    var magnitude:u32;
    if exponent >= 134u { magnitude = significand << (exponent - 134u); }
    else { let shift = 134u - exponent; magnitude = (significand + (1u << (shift - 1u))) >> shift; }
    if (bits >> 31u) != 0u { return -i32(magnitude); }
    return i32(magnitude);
}

fn load_texel(x:u32) -> u32 {
    if params.value.w == 0u { return source[params.source_offset/4u+x]; }
    let bytes = vec4<u32>(floor(textureLoad(texels, vec2<i32>(i32(x),0), 0)*255.0+0.5));
    return bytes.x | (bytes.y<<8u) | (bytes.z<<16u) | (bytes.w<<24u);
}

@compute @workgroup_size(64)
fn sample_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x >= params.count {return;}
    let position = coordinate(params.value.x) + i32(id.x) * coordinate(params.value.y);
    let base = position >> 16u;
    let fraction = position & 65535;
    let left = u32(clamp(base, 0, i32(params.value.z - 1u)));
    let right = u32(clamp(base + 1, 0, i32(params.value.z - 1u)));
    let a=load_texel(left);
    let b=load_texel(right);
    var packed=0u;
    for(var lane=0u;lane<4u;lane++) {
        let low = i32((a >> (lane * 8u)) & 255u);
        let high = i32((b >> (lane * 8u)) & 255u);
        let value = (low * 65536 + (high - low) * fraction + 32768) >> 16u;
        packed |= u32(value) << (lane * 8u);
    }
    destination[params.destination_offset/4u+id.x]=packed;
}
