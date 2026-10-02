#include "../constants.hlsli"
#include "../shared/pixel.hlsli"
#include "../shared/texture_table_constants.hlsli"
Texture2D<float4> texture_table[NATIVE_TEXTURE_TABLE_CAPACITY]:register(t30);
ByteAddressBuffer requests:register(t0);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> output:register(u1);
[numthreads(FINE_WORKGROUP_SIZE,1,1)]
void texture_table_words(uint3 id:SV_DispatchThreadID) {
    uint bytes; requests.GetDimensions(bytes);
    if(id.x>=bytes/12u)return;
    uint3 request=requests.Load3(id.x*12u);
    output[uint2(id.x,0)]=texture_table[NonUniformResourceIndex(request.x)].Load(int3(request.yz,0));
}
