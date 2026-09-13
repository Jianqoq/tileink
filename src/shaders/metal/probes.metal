#include <metal_stdlib>
using namespace metal;

// Independent MSL implementation of the same integer probe/32-byte ABI.
struct ProbeParams {
    uint count;
    uint source_offset;
    uint destination_offset;
    uint stride;
    uint4 value;
};

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

// Independent source; requires real Apple compiler/GPU verification before acceptance.
kernel void sample_words(device uint* destination [[buffer(0)]],
    const device uint* source [[buffer(1)]], constant ProbeParams& params [[buffer(2)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= params.count) return;
    float position = float(id) * as_type<float>(params.value.y) + as_type<float>(params.value.x);
    float base = floor(position);
    float fraction = position - base;
    uint left = uint(clamp(base, 0.0f, float(params.value.z - 1)));
    uint right = uint(clamp(base + 1.0f, 0.0f, float(params.value.z - 1)));
    uint a = source[params.source_offset / 4 + left];
    uint b = source[params.source_offset / 4 + right];
    uint packed = 0;
    for (uint lane = 0; lane < 4; ++lane) {
        float low = float((a >> (lane * 8)) & 255);
        float high = float((b >> (lane * 8)) & 255);
        packed |= uint(floor(low + (high - low) * fraction + 0.5f)) << (lane * 8);
    }
    destination[params.destination_offset / 4 + id] = packed;
}
