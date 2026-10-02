#include "constants.hlsli"
#include "probe_abi.hlsli"
#include "probe_sampling.hlsli"

RWByteAddressBuffer destination : register(u0, space0);
ByteAddressBuffer source : register(t1, space0);
ConstantBuffer<ProbeParams> params : register(b2, space0);

Texture2D<float4> texels : register(t3, space0);

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

[numthreads(64, 1, 1)]
void sample_words(uint3 id : SV_DispatchThreadID) {
    if (id.x >= params.count) return;
    int position = q16_coordinate(params.value.x) + int(id.x) * q16_coordinate(params.value.y);
    int base = position >> 16;
    int fraction = position & 65535;
    uint left = uint(clamp(base, 0, int(params.value.z - 1)));
    uint right = uint(clamp(base + 1, 0, int(params.value.z - 1)));
    uint a = probe_load_texel(source, texels, params, left);
    uint b = probe_load_texel(source, texels, params, right);
    uint packed = 0;
    for (uint lane = 0; lane < 4; ++lane) {
        int low = int((a >> (lane * 8)) & 255);
        int high = int((b >> (lane * 8)) & 255);
        uint channel = uint((low * 65536 + (high - low) * fraction + 32768) >> 16);
        packed |= channel << (lane * 8);
    }
    destination.Store(params.destination_offset + id.x * 4, packed);
}
