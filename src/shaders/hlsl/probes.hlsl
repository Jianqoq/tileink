#include "constants.hlsli"
#include "probe_abi.hlsli"

[numthreads(64, 1, 1)]
void clear_words(uint3 id : SV_DispatchThreadID) {
    if (id.x < params.count)
        destination.Store(params.destination_offset + id.x * 4, params.value.x);
}

[numthreads(64, 1, 1)]
void copy_words(uint3 id : SV_DispatchThreadID) {
    if (id.x < params.count)
        destination.Store(params.destination_offset + id.x * 4,
            source.Load(params.source_offset + id.x * 4));
}

[numthreads(64, 1, 1)]
void layout_words(uint3 id : SV_DispatchThreadID) {
    if (id.x < params.count)
        destination.Store4(params.destination_offset + id.x * params.stride,
            params.value + uint4(id.x, params.source_offset, params.stride, params.count));
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

// Explicit taps avoid API-dependent fixed-function interpolation precision.
uint load_texel(uint x) {
    if (params.value.w == 0) return source.Load(params.source_offset + x * 4);
    uint4 channels = uint4(floor(texels.Load(int3(x, 0, 0)) * 255.0 + 0.5));
    return channels.x | (channels.y << 8) | (channels.z << 16) | (channels.w << 24);
}
[numthreads(64, 1, 1)]
void sample_words(uint3 id : SV_DispatchThreadID) {
    if (id.x >= params.count) return;
    int position = coordinate(params.value.x) + int(id.x) * coordinate(params.value.y);
    int base = position >> 16;
    int fraction = position & 65535;
    uint left = uint(clamp(base, 0, int(params.value.z - 1)));
    uint right = uint(clamp(base + 1, 0, int(params.value.z - 1)));
    uint a = load_texel(left);
    uint b = load_texel(right);
    uint packed = 0;
    for (uint lane = 0; lane < 4; ++lane) {
        int low = int((a >> (lane * 8)) & 255);
        int high = int((b >> (lane * 8)) & 255);
        uint channel = uint((low * 65536 + (high - low) * fraction + 32768) >> 16);
        packed |= channel << (lane * 8);
    }
    destination.Store(params.destination_offset + id.x * 4, packed);
}
