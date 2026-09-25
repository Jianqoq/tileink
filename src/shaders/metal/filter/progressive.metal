#include <metal_stdlib>
using namespace metal;

struct ProgressiveBlurConfig {
    uint4 output;
    uint4 source;
    float4 gradient;
    float max_std_dev;
    uint count;
    uint step;
    uint axis;
};
struct ProgressiveTextureTable { array<texture2d<float>, 64> images [[id(0)]]; };

float4 progressive_read(texture2d<float> source, int2 p, uint2 size) {
    if (any(p < 0) || any(p >= int2(size))) return 0;
    return source.read(uint2(p));
}

float4 progressive_sample_source(texture2d<float> source, sampler sampling, float2 p, uint2 size) {
    if (all(p >= 0) && all(p <= float2(size) - 1.0f))
        return source.sample(sampling, (p + 0.5f) / float2(source.get_width(), source.get_height()), level(0));
    int2 base = int2(floor(p));
    float2 f = fract(p);
    return mix(mix(progressive_read(source, base, size), progressive_read(source, base + int2(1, 0), size), f.x),
        mix(progressive_read(source, base + int2(0, 1), size), progressive_read(source, base + int2(1, 1), size), f.x), f.y);
}

kernel void progressive_blur_reduce(constant ProgressiveBlurConfig& config [[buffer(0)]],
    texture2d<float> source_texture [[texture(1)]],
    const device float2* level_metadata [[buffer(2)]],
    texture2d<float, access::write> target_texture [[texture(3)]],
    sampler image_sampler [[sampler(13)]], uint3 id [[thread_position_in_grid]]) {
    if (any(id.xy >= config.output.zw)) return;
    float2 center = float2(id.xy);
    center[config.axis] *= config.step;
    float4 color = 0;
    for (uint i = 0; i < config.count; ++i) {
        float2 tap = level_metadata[i];
        float2 p = center;
        p[config.axis] += tap.x;
        color += tap.y * progressive_sample_source(source_texture, image_sampler, p, config.source.zw);
    }
    target_texture.write(color, id.xy);
}

float4 progressive_read_level(constant ProgressiveTextureTable& table, uint index, int2 p, uint2 size) {
    if (any(p < 0) || any(p >= int2(size))) return 0;
    return table.images[index].read(uint2(p));
}

float4 progressive_sample(constant ProgressiveTextureTable& table, uint index, float2 p, float4 metadata, sampler sampling) {
    float2 position = p / metadata.z - 0.5f;
    if (all(position >= 0) && all(position <= metadata.xy - 1.0f))
        return table.images[index].sample(sampling, (position + 0.5f) /
            float2(table.images[index].get_width(), table.images[index].get_height()), level(0));
    int2 base = int2(floor(position));
    float2 f = fract(position);
    uint2 size = uint2(metadata.xy);
    return mix(mix(progressive_read_level(table, index, base, size),
        progressive_read_level(table, index, base + int2(1, 0), size), f.x),
        mix(progressive_read_level(table, index, base + int2(0, 1), size),
        progressive_read_level(table, index, base + int2(1, 1), size), f.x), f.y);
}

float4 progressive_shallow(constant ProgressiveTextureTable& table, float2 p, float sigma,
    float4 metadata, sampler sampling) {
    float weights[7];
    float total = 0;
    for (int i = 0; i < 7; ++i) {
        float d = float(i - 3);
        weights[i] = exp(-0.5f * d * d / (sigma * sigma));
        total += weights[i];
    }
    float offsets[4], combined[4];
    for (int i = 0; i < 4; ++i) {
        int first = i * 2;
        float second = first + 1 < 7 ? weights[first + 1] : 0;
        combined[i] = weights[first] + second;
        offsets[i] = float(first - 3) + (combined[i] > 0 ? second / combined[i] : 0);
    }
    float4 color = 0;
    for (int y = 0; y < 4; ++y)
        for (int x = 0; x < 4; ++x)
            color += combined[x] * combined[y] * progressive_sample(table, 0, p + float2(offsets[x], offsets[y]), metadata, sampling);
    return color / (total * total);
}

kernel void progressive_blur_resolve(constant ProgressiveBlurConfig& config [[buffer(0)]],
    const device float4* level_metadata [[buffer(2)]],
    texture2d<float, access::write> target_texture [[texture(3)]],
    constant ProgressiveTextureTable& texture_table [[buffer(30)]],
    sampler image_sampler [[sampler(13)]],
    uint3 id [[thread_position_in_grid]]) {
    if (any(id.xy >= config.output.zw)) return;
    uint2 xy = config.output.xy + id.xy;
    float t = all(config.gradient.zw == 0) ? 1.0f
        : clamp(dot(float2(xy) + 0.5f - config.gradient.xy, config.gradient.zw), 0.0f, 1.0f);
    float sigma = config.max_std_dev * t * t * (3.0f - 2.0f * t);
    if (sigma == 0) return;
    float2 p = float2(xy - config.source.xy) + 0.5f;
    if (sigma < 0.125f) return;
    if (sigma <= 1.0f) {
        target_texture.write(progressive_shallow(texture_table, p, sigma, level_metadata[0], image_sampler), xy);
        return;
    }
    float variance = sigma * sigma;
    uint upper = 1;
    float4 high = level_metadata[upper];
    while (upper + 1 < config.count && high.w < variance) {
        ++upper;
        high = level_metadata[upper];
    }
    float4 low = level_metadata[upper - 1];
    float weight = clamp((variance - low.w) / (high.w - low.w), 0.0f, 1.0f);
    target_texture.write(mix(progressive_sample(texture_table, upper - 1, p, low, image_sampler),
        progressive_sample(texture_table, upper, p, high, image_sampler), weight), xy);
}
