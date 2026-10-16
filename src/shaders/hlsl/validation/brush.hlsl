#include "../shared/texture_table_constants.hlsli"
#include "../constants.hlsli"
#include "../fine/config.hlsli"
#include "../shared/brush/sample.hlsli"
ConstantBuffer<FineConfig> config : register(b0);
struct RequestConfig { uint count; uint pad0; uint pad1; uint pad2; };
ConstantBuffer<RequestConfig> request_config : register(b11);
ByteAddressBuffer paint : register(t3);
ByteAddressBuffer requests : register(t9);
RWByteAddressBuffer output : register(u10);
Texture2DArray<float4> image_resource_atlas : register(t12);
SamplerState image_resource_sampler : register(s13);
Texture2D<float4> image_resource_textures[NATIVE_TEXTURE_TABLE_CAPACITY] : register(t30);
[numthreads(FINE_WORKGROUP_SIZE,1,1)]
void brush_words(uint3 id:SV_DispatchThreadID) {
    if(id.x>=request_config.count) return;
    uint3 request=requests.Load3(id.x*16u);
    output.Store(id.x*4u,sample_brush(paint,config.paint_brush_base,request.x,image_resource_atlas,image_resource_sampler,image_resource_textures,asfloat(request.y),asfloat(request.z)));
}
