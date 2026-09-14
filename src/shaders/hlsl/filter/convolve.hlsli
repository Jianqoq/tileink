#ifndef TILEINK_FILTER_CONVOLVE_HLSLI
#define TILEINK_FILTER_CONVOLVE_HLSLI
#include "config.hlsli"
#include "../shared/pixel.hlsli"
#include "../shared/integer.hlsli"

// Signed remainder maps directly into the primitive region, even for kernels
// wider than that region. No repeated wrap loop or implicit robust load is used.
int convolve_wrap(int coordinate, int origin, int length) {
    return origin+int(euclidean_remainder_i32(coordinate-origin,uint(length)));
}
uint filter_convolve_pixel(ConstantBuffer<FilterConfig> config, Texture2D<float4> source,
    ByteAddressBuffer kernels, uint2 xy) {
    uint center=unorm_to_rgba8(source.Load(int3(xy,0)));
    if (config.kernel_columns==0u || config.kernel_rows==0u || config.amount==0.0) return center;
    int2 lower=int2(config.region_x0,config.region_y0);
    int2 size=int2(config.region_width,config.region_height);
    int2 upper=lower+size;
    float4 sum=0.0;
    for (uint ky=0u; ky<config.kernel_rows; ++ky) {
        for (uint kx=0u; kx<config.kernel_columns; ++kx) {
            uint index=config.kernel_offset+(config.kernel_rows-1u-ky)*config.kernel_columns
                +(config.kernel_columns-1u-kx);
            float weight=asfloat(kernels.Load(index*4u));
            int2 position=int2(xy)+int2(kx,ky)-int2(config.kernel_target_x,config.kernel_target_y);
            uint pixel=0u;
            if (config.kernel_edge_mode==1u) {
                position=clamp(position,lower,upper-1);
                pixel=unorm_to_rgba8(source.Load(int3(position,0)));
            } else if (config.kernel_edge_mode==2u) {
                position=int2(convolve_wrap(position.x,lower.x,size.x),convolve_wrap(position.y,lower.y,size.y));
                pixel=unorm_to_rgba8(source.Load(int3(position,0)));
            } else if (all(position>=lower) && all(position<upper)) {
                pixel=unorm_to_rgba8(source.Load(int3(position,0)));
            }
            uint alpha=pixel>>24u;
            float4 value=float4(straight_channel(pixel&255u,alpha),straight_channel((pixel>>8u)&255u,alpha),
                straight_channel((pixel>>16u)&255u,alpha),float(alpha)/255.0);
            sum+=value*weight;
        }
    }
    float4 result=clamp(sum/config.amount+config.rect_x0,0.0,1.0);
    if (config.kernel_preserve_alpha==1u) result.a=float(center>>24u)/255.0;
    return pack_premul_rgba8(result.r*result.a,result.g*result.a,result.b*result.a,result.a);
}
#endif
