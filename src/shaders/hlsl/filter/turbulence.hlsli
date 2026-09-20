#ifndef TILEINK_FILTER_TURBULENCE_HLSLI
#define TILEINK_FILTER_TURBULENCE_HLSLI
#include "config.hlsli"
#include "turbulence_noise.hlsli"
#include "color_space.hlsli"
#include "../shared/pixel.hlsli"
float4 turbulence_channels(ByteAddressBuffer selectors,ByteAddressBuffer gradients,uint table,float2 position,bool stitch,int2 wrap,int2 extent,float rounding_zero) {
    return float4(turbulence_noise(selectors,gradients,table,0u,position,stitch,wrap,extent,rounding_zero),turbulence_noise(selectors,gradients,table,1u,position,stitch,wrap,extent,rounding_zero),turbulence_noise(selectors,gradients,table,2u,position,stitch,wrap,extent,rounding_zero),turbulence_noise(selectors,gradients,table,3u,position,stitch,wrap,extent,rounding_zero));
}
uint turbulence_pack(float4 value,uint kind,uint linear_rgb) {
    if(kind==1u) value=value*0.5+0.5;
    value=clamp(value,0.0,1.0);
    if(linear_rgb==1u) {
        value.r=filter_linear_rgb_to_srgb(value.r);
        value.g=filter_linear_rgb_to_srgb(value.g);
        value.b=filter_linear_rgb_to_srgb(value.b);
    }
    return pack_premul_rgba8(value.r*value.a,value.g*value.a,value.b*value.a,value.a);
}
uint turbulence_pixel(ConstantBuffer<FilterConfig> config,ByteAddressBuffer selectors,ByteAddressBuffer gradients,float2 position) {
    float2 scale=float2(config.turbulence_scale_x,config.turbulence_scale_y);
    if(any(abs(scale)<=TURBULENCE_SCALE_EPSILON)) return 0u;
    if(config.turbulence_num_octaves==0u || (config.turbulence_base_frequency_x==0.0 && config.turbulence_base_frequency_y==0.0)) return turbulence_pack(0.0,config.turbulence_kind,config.turbulence_linear_rgb);
    float2 sample_base=(position-float2(config.turbulence_transform_x,config.turbulence_transform_y))/scale;

    float2 frequency=float2(config.turbulence_base_frequency_x,config.turbulence_base_frequency_y);
    bool stitch=config.turbulence_stitch_tiles==1u;
    int2 extent=0,wrap=0;
    if(stitch) {
        // Stitch boundaries are fixed in noise space, independent of the sampled pixel.
        float2 tile_origin=(float2(config.turbulence_tile_x,config.turbulence_tile_y)-float2(config.turbulence_transform_x,config.turbulence_transform_y))/scale;
        float2 tile_delta=float2(config.turbulence_tile_width,config.turbulence_tile_height)/scale;
        float2 tile_lower=min(tile_origin,tile_origin+tile_delta);
        float2 tile=abs(tile_delta);
        frequency=float2(turbulence_stitch_frequency(frequency.x,tile.x),turbulence_stitch_frequency(frequency.y,tile.y));
        extent=int2(tile*frequency+0.5);
        wrap=int2(floor(tile_lower*frequency+TURBULENCE_COORDINATE_OFFSET+float2(extent)));
    }
    float ratio=1.0;float4 value=0.0;
    for(uint octave=0u;octave<config.turbulence_num_octaves;++octave) {
        float4 noise=turbulence_channels(selectors,gradients,config.table_index,sample_base*frequency,stitch,wrap,extent,config.rounding_zero);
        value+=(config.turbulence_kind==0u ? abs(noise) : noise)*ratio;
        if(octave+1u>=config.turbulence_num_octaves) break;
        ratio*=0.5;
        if(ratio==0.0) break;
        frequency*=2.0;
        if(stitch) {extent*=2;wrap=2*wrap-int(TURBULENCE_COORDINATE_OFFSET);}
    }
    return turbulence_pack(value,config.turbulence_kind,config.turbulence_linear_rgb);
}
#endif
