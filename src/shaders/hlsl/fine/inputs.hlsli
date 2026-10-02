#ifndef TILEINK_HLSL_FINE_INPUTS_INCLUDED
#define TILEINK_HLSL_FINE_INPUTS_INCLUDED
#include "config.hlsli"

// Resources are passed explicitly; texture arrays remain separate because SPIR-V
// forbids opaque arrays inside structures. Config is a value, never a CB handle.
struct FineInputs {
    FineConfig config;
    ByteAddressBuffer draws, paint, segments, text;
    RWByteAddressBuffer coarse, spills;
    RWTexture2D<float4> target;
    Texture2DArray<float4> atlas;
    SamplerState image_sampler;
};

#endif
