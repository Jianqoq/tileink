#ifndef TILEINK_FILTER_TRANSFER_HLSLI
#define TILEINK_FILTER_TRANSFER_HLSLI
#include "../constants.hlsli"
#include "../shared/pixel.hlsli"
uint filter_straight_component_index(uint premul,uint alpha) {
    return alpha==0u ? 0u : min((premul*255u+alpha/2u)/alpha,255u);
}
uint filter_transfer_pixel(ByteAddressBuffer tables,uint index,uint pixel) {
    uint alpha=pixel>>24u;
    uint base=index*COMPONENT_TRANSFER_TABLE_LEN;
    uint r_index=filter_straight_component_index(pixel&255u,alpha);
    uint g_index=filter_straight_component_index((pixel>>8u)&255u,alpha);
    uint b_index=filter_straight_component_index((pixel>>16u)&255u,alpha);
    float r=float(tables.Load((base+r_index)*4u))*CHANNEL_SCALE;
    float g=float(tables.Load((base+COMPONENT_TRANSFER_TABLE_SIZE+g_index)*4u))*CHANNEL_SCALE;
    float b=float(tables.Load((base+2u*COMPONENT_TRANSFER_TABLE_SIZE+b_index)*4u))*CHANNEL_SCALE;
    float a=float(tables.Load((base+3u*COMPONENT_TRANSFER_TABLE_SIZE+alpha)*4u))*CHANNEL_SCALE;
    return pack_premul_rgba8(r*a,g*a,b*a,a);
}
#endif
