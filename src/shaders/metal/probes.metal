#include <metal_stdlib>
using namespace metal;

#include "probe_params.metal"

kernel void clear_words(device uint* destination [[buffer(0)]],
    constant ProbeParams& params [[buffer(2)]], uint id [[thread_position_in_grid]]) {
    if (id < params.count) destination[params.destination_offset / 4 + id] = params.value.x;
}

kernel void copy_words(device uint* destination [[buffer(0)]],
    const device uint* source [[buffer(1)]], constant ProbeParams& params [[buffer(2)]],
    uint id [[thread_position_in_grid]]) {
    if (id < params.count)
        destination[params.destination_offset / 4 + id] = source[params.source_offset / 4 + id];
}

kernel void layout_words(device uint* destination [[buffer(0)]],
    constant ProbeParams& params [[buffer(2)]], uint id [[thread_position_in_grid]]) {
    if (id < params.count) {
        uint4 value = params.value + uint4(id, params.source_offset, params.stride, params.count);
        uint offset = (params.destination_offset + id * params.stride) / 4;
        for (uint lane = 0; lane < 4; ++lane) destination[offset + lane] = value[lane];
    }
}

// Canonical Q16 coordinates: decode binary32 bits and round magnitude ties
// upward. Integer interpolation prevents compiler FMA/reassociation changing a
// final RGBA8 channel by one. This is the numerical contract, not a tolerance.
int coordinate(uint bits) {
    uint exponent = (bits >> 23) & 255;
    if (exponent <= 109) return 0;
    uint significand = (bits & 0x7fffff) | 0x800000;
    uint magnitude;
    if (exponent >= 134) magnitude = significand << (exponent - 134);
    else { uint shift = 134 - exponent; magnitude = (significand + (1u << (shift - 1))) >> shift; }
    return (bits >> 31) != 0 ? -int(magnitude) : int(magnitude);
}

uint read_texel(const device uint* source, constant ProbeParams& params,
    texture2d<float, access::read> texels, uint x) {
    if (params.value.w == 0) return source[params.source_offset / 4 + x];
    uint4 bytes = uint4(floor(texels.read(uint2(x, 0)) * 255.0f + 0.5f));
    return bytes.x | (bytes.y << 8) | (bytes.z << 16) | (bytes.w << 24);
}

// Same-device clear/copy/layout/sampling acceptance: tests/metal_probes.rs.
kernel void sample_words(device uint* destination [[buffer(0)]],
    const device uint* source [[buffer(1)]], constant ProbeParams& params [[buffer(2)]],
    texture2d<float, access::read> texels [[texture(3)]], uint id [[thread_position_in_grid]]) {
    if (id >= params.count) return;
    int position = coordinate(params.value.x) + int(id) * coordinate(params.value.y);
    int base = position >> 16;
    int fraction = position & 65535;
    uint left = uint(clamp(base, 0, int(params.value.z - 1)));
    uint right = uint(clamp(base + 1, 0, int(params.value.z - 1)));
    uint a = read_texel(source, params, texels, left);
    uint b = read_texel(source, params, texels, right);
    uint packed = 0;
    for (uint lane = 0; lane < 4; ++lane) {
        int low = int((a >> (lane * 8)) & 255);
        int high = int((b >> (lane * 8)) & 255);
        packed |= uint((low * 65536 + (high - low) * fraction + 32768) >> 16) << (lane * 8);
    }
    destination[params.destination_offset / 4 + id] = packed;
}
