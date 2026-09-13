#ifndef TILEINK_HLSL_PROBE_SAMPLING_HLSLI_INCLUDED
#define TILEINK_HLSL_PROBE_SAMPLING_HLSLI_INCLUDED

#include "probe_abi.hlsli"

// Canonical Q16 coordinates: decode binary32 bits and round magnitude ties
// upward. Integer interpolation prevents compiler FMA/reassociation changing a
// final RGBA8 channel by one. This is the numerical contract, not a tolerance.
int q16_coordinate(uint bits) {
    uint exponent = (bits >> 23) & 255;
    if (exponent <= 109) return 0;
    uint significand = (bits & 0x7fffff) | 0x800000;
    uint magnitude;
    if (exponent >= 134) magnitude = significand << (exponent - 134);
    else { uint shift = 134 - exponent; magnitude = (significand + (1u << (shift - 1))) >> shift; }
    return (bits >> 31) != 0 ? -int(magnitude) : int(magnitude);
}

// Explicit taps avoid API-dependent fixed-function interpolation precision.
uint probe_load_texel(ByteAddressBuffer input, Texture2D<float4> image_texture, ConstantBuffer<ProbeParams> settings, uint x) {
    if (settings.value.w == 0) return input.Load(settings.source_offset + x * 4);
    uint4 channels = uint4(floor(image_texture.Load(int3(x, 0, 0)) * 255.0 + 0.5));
    return channels.x | (channels.y << 8) | (channels.z << 16) | (channels.w << 24);
}

#endif // TILEINK_HLSL_PROBE_SAMPLING_HLSLI_INCLUDED
