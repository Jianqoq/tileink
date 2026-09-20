#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "turbulence/noise.metal"
float noise_srgb(float value) {return value>0.0031308f?1.055f*pow(value,1.0f/2.4f)-0.055f:value*12.92f;}
uint turbulence_pixel(constant FilterConfig& c,const device uint* selectors,const device float2* gradients,float2 position) {
    float2 scale(c.turbulence_scale_x,c.turbulence_scale_y);
    if(any(abs(scale)<=0.00000011920929f)) return 0;
    float4 accumulated=0;
    if(c.turbulence_num_octaves && (c.turbulence_base_frequency_x!=0 || c.turbulence_base_frequency_y!=0)) {
        float2 transform(c.turbulence_transform_x,c.turbulence_transform_y),sample=(position-transform)/scale;
        float2 frequency(c.turbulence_base_frequency_x,c.turbulence_base_frequency_y);
        bool stitch=c.turbulence_stitch_tiles==1;
        int2 extent=0,wrap=0;
        if(stitch) {
            float2 origin=(float2(c.turbulence_tile_x,c.turbulence_tile_y)-transform)/scale;
            float2 delta=float2(c.turbulence_tile_width,c.turbulence_tile_height)/scale,size=abs(delta),lower=min(origin,origin+delta);
            frequency=float2(stitch_frequency(frequency.x,size.x),stitch_frequency(frequency.y,size.y));
            extent=int2(size*frequency+0.5f);wrap=int2(floor(lower*frequency+4096.0f+float2(extent)));
        }
        float amplitude=1;
        for(uint octave=0;octave<c.turbulence_num_octaves;++octave) {
            float4 value;
            for(uint channel=0;channel<4;++channel) value[channel]=perlin(selectors,gradients,c.table_index,channel,sample*frequency,stitch,wrap,extent,c.rounding_zero);
            accumulated+=(c.turbulence_kind==0?abs(value):value)*amplitude;
            if(octave+1>=c.turbulence_num_octaves) break;
            amplitude*=0.5f;if(amplitude==0) break;
            frequency*=2.0f;
            if(stitch) {extent*=2;wrap=2*wrap-4096;}
        }
    }
    if(c.turbulence_kind==1) accumulated=accumulated*0.5f+0.5f;
    accumulated=clamp(accumulated,0.0f,1.0f);
    if(c.turbulence_linear_rgb==1) accumulated.rgb=float3(noise_srgb(accumulated.r),noise_srgb(accumulated.g),noise_srgb(accumulated.b));
    return pack_pixel(float4(accumulated.rgb*accumulated.a,accumulated.a));
}
kernel void filter_turbulence_region(constant FilterConfig& config [[buffer(0)]],texture2d<float,access::write> target [[texture(3)]],
    const device uint* selectors [[buffer(5)]],const device float2* gradients [[buffer(6)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(filter_position(config,tiles,id,xy)) target.write(unpack_pixel(turbulence_pixel(config,selectors,gradients,float2(xy))),xy);
}
