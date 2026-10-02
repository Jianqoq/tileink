#include "../constants.hlsli"
#include "../fine/config.hlsli"
#include "../shared/brush/pattern.hlsli"
ConstantBuffer<FineConfig> config : register(b0);
struct RequestConfig { uint count; uint pad0; uint pad1; uint pad2; };
ConstantBuffer<RequestConfig> request_config : register(b11);
ByteAddressBuffer paint : register(t3);
ByteAddressBuffer requests : register(t9);
RWByteAddressBuffer output : register(u10);
Texture2DArray<float4> image_resource_atlas : register(t12);
SamplerState image_resource_sampler : register(s13);
[numthreads(FINE_WORKGROUP_SIZE,1,1)]
void pattern_words(uint3 id : SV_DispatchThreadID) {
    if (id.x >= request_config.count) return;
    uint3 record = requests.Load3(id.x*16u);
    uint color = sample_atlas_brush(paint, config.paint_brush_base, record.x,
        image_resource_atlas, image_resource_sampler, asfloat(record.y), asfloat(record.z));
    output.Store(id.x*4u, color);
}
