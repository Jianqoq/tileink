#include "../constants.hlsli"
#include "../shared/pixel.hlsli"
struct SamplerConfig { uint count; uint pad0; uint pad1; uint pad2; };
ConstantBuffer<SamplerConfig> config : register(b0);
struct DispatchGrid { uint x; uint y; uint z; uint _pad; };
ConstantBuffer<DispatchGrid> dispatch_grid : register(b31);
Texture2DArray<float4> source : register(t1);
SamplerState image_sampler : register(s2);
ByteAddressBuffer requests : register(t3);
RWByteAddressBuffer destination : register(u4);
[numthreads(FINE_WORKGROUP_SIZE,1,1)]
void sampler_words(uint3 id:SV_DispatchThreadID) {
    uint index = id.x + id.y * dispatch_grid.x * FINE_WORKGROUP_SIZE;
    if (index >= config.count) return;
    uint4 request = requests.Load4(index*16u);
    float4 pixel = source.SampleLevel(image_sampler, float3(asfloat(request.xy), float(request.z)), 0.0);
    destination.Store(index*4u, unorm_to_rgba8(pixel));
}
