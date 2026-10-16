#include "../constants.hlsli"
struct TextureConfig { uint width; uint height; uint layer; uint pad1; };
ConstantBuffer<TextureConfig> config : register(b0);
Texture2DArray<float4> source : register(t1);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> destination : register(u2);
[numthreads(FINE_WORKGROUP_SIZE, 1, 1)]
void texture_layer(uint3 id : SV_DispatchThreadID) {
    if (id.x >= config.width || id.y >= config.height) return;
    destination[id.xy] = source.Load(int4(id.xy, config.layer, 0));
}
