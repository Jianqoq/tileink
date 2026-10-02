#include "../constants.hlsli"
struct TextureConfig { uint width; uint height; uint pad0; uint pad1; };
ConstantBuffer<TextureConfig> config : register(b0);
Texture2D<float4> source : register(t1);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> destination : register(u2);
[numthreads(FINE_WORKGROUP_SIZE, 1, 1)]
void texture_flip(uint3 id : SV_DispatchThreadID) {
    if (id.x >= config.width || id.y >= config.height) return;
    destination[id.xy] = source.Load(int3(config.width - 1u - id.x, config.height - 1u - id.y, 0)).bgra;
}
