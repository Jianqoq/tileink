#include "../shared/texture_table_constants.hlsli"
struct ProgressiveBlurConfig {
    uint4 output;
    uint4 source;
    float4 gradient;
    float max_std_dev;
    uint level_count;
    uint step;
    uint pad;
};
ConstantBuffer<ProgressiveBlurConfig> config : register(b0);
Texture2D<float4> source_texture : register(t1);
ByteAddressBuffer level_metadata : register(t2);
SamplerState image_sampler : register(s13);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture : register(u3);
Texture2D<float4> texture_table[NATIVE_TEXTURE_TABLE_CAPACITY] : register(t30);

float4 read_source(int2 p) {
    if (any(p < 0) || any(p >= int2(config.source.zw))) return 0;
    return source_texture.Load(int3(p, 0));
}

float4 sample_source(float2 p) {
    if (all(p >= 0) && all(p <= float2(config.source.zw) - 1.0)) {
        uint width, height;
        source_texture.GetDimensions(width, height);
        return source_texture.SampleLevel(image_sampler, (p + 0.5) / float2(width, height), 0);
    }
    // Clamp samplers must not extend edge colors or read spare pooled capacity.
    int2 base = int2(floor(p));
    float2 f = frac(p);
    return lerp(lerp(read_source(base), read_source(base + int2(1, 0)), f.x),
        lerp(read_source(base + int2(0, 1)), read_source(base + int2(1, 1)), f.x), f.y);
}

[numthreads(8, 8, 1)]
void progressive_blur_reduce(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= config.output.zw)) return;
    // Four bilinear taps exactly factor the 3x3 or 4x4 binomial weights.
    float2 center = float2(id.xy) * float(config.step) + (config.step == 1 ? 0.0 : 0.5);
    float offset = config.step == 1 ? 0.5 : 0.75;
    target_texture[id.xy] = 0.25 * (
        sample_source(center + float2(-offset, -offset)) + sample_source(center + float2(offset, -offset))
        + sample_source(center + float2(-offset, offset)) + sample_source(center + float2(offset, offset)));
}

float4 read_level(uint index, int2 p, uint2 size) {
    if (any(p < 0) || any(p >= int2(size))) return 0;
    return texture_table[NonUniformResourceIndex(index)].Load(int3(p, 0));
}

float4 sample_level(uint index, float2 p, float4 metadata) {
    float2 position = p / metadata.z - 0.5;
    if (all(position >= 0) && all(position <= metadata.xy - 1.0)) {
        uint width, height;
        texture_table[NonUniformResourceIndex(index)].GetDimensions(width, height);
        return texture_table[NonUniformResourceIndex(index)].SampleLevel(image_sampler,
            (position + 0.5) / float2(width, height), 0);
    }
    int2 base = int2(floor(position));
    float2 f = frac(position);
    uint2 size = uint2(metadata.xy);
    return lerp(lerp(read_level(index, base, size), read_level(index, base + int2(1, 0), size), f.x),
        lerp(read_level(index, base + int2(0, 1), size), read_level(index, base + int2(1, 1), size), f.x), f.y);
}

[numthreads(8, 8, 1)]
void progressive_blur_resolve(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= config.output.zw)) return;
    uint2 xy = config.output.xy + id.xy;
    float t = all(config.gradient.zw == 0) ? 1.0
        : saturate(dot(float2(xy) + 0.5 - config.gradient.xy, config.gradient.zw));
    float sigma = config.max_std_dev * t * t * (3.0 - 2.0 * t);
    if (sigma == 0) return; // Preserve exactly clear pixels, including their alpha.
    float variance = sigma * sigma;
    uint upper = 1;
    float4 high = asfloat(level_metadata.Load4(upper * 16));
    while (upper + 1 < config.level_count && high.w < variance) {
        ++upper;
        high = asfloat(level_metadata.Load4(upper * 16));
    }
    float4 low = asfloat(level_metadata.Load4((upper - 1) * 16));
    float weight = saturate((variance - low.w) / (high.w - low.w));
    float2 p = float2(xy - config.source.xy) + 0.5;
    target_texture[xy] = lerp(sample_level(upper - 1, p, low), sample_level(upper, p, high), weight);
}
