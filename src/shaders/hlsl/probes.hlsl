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

// Manual texel interpolation isolates arithmetic/quantization from hardware samplers.
[numthreads(64, 1, 1)]
void sample_words(uint3 id : SV_DispatchThreadID) {
    if (id.x >= params.count) return;
    precise float scaled = float(id.x) * asfloat(params.value.y);
    precise float position = scaled + asfloat(params.value.x);
    float base = floor(position);
    precise float fraction = position - base;
    uint left = uint(clamp(base, 0.0, float(params.value.z - 1)));
    uint right = uint(clamp(base + 1.0, 0.0, float(params.value.z - 1)));
    uint a = source.Load(params.source_offset + left * 4);
    uint b = source.Load(params.source_offset + right * 4);
    uint packed = 0;
    for (uint lane = 0; lane < 4; ++lane) {
        precise float low = float((a >> (lane * 8)) & 255);
        precise float delta = float((b >> (lane * 8)) & 255) - low;
        precise float weighted = delta * fraction;
        precise float interpolated = low + weighted;
        uint channel = uint(floor(interpolated + 0.5));
        packed |= channel << (lane * 8);
    }
    destination.Store(params.destination_offset + id.x * 4, packed);
}
