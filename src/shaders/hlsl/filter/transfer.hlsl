#include "region.hlsli"
#include "transfer.hlsli"
#include "../shared/pixel.hlsli"
ConstantBuffer<FilterConfig> config : register(b0);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture : register(u3);
ByteAddressBuffer transfer_tables : register(t7);
ByteAddressBuffer active_tiles : register(t8);
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_component_transfer_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (filter_position(config,active_tiles,gid,xy)) {
        uint pixel=unorm_to_rgba8(target_texture[xy]);
        target_texture[xy]=rgba8_to_unorm(filter_transfer_pixel(transfer_tables,config.table_index,pixel));
    }
}
