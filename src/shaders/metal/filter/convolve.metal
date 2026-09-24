#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "color_space.metal"
// Rescale through exponent bits so subnormal divisors survive flush-to-zero;
// inverse rescaling also prevents large normal divisors losing their reciprocal.
float exponent_scale(float value,bool up) {
    uint bits=as_type<uint>(value),magnitude=bits&0x7fffffffu,exponent=magnitude>>23;
    if(!up) return exponent<=24?0.0f:as_type<float>(bits-(24u<<23));
    if(!exponent) {float result=float(magnitude)*as_type<float>(0x01000000u);return bits>>31?-result:result;}
    return exponent>=231?as_type<float>((bits&0x80000000u)|0x7f800000u):as_type<float>(bits+(24u<<23));
}
int wrap_convolve(int p,int origin,int length) {int remainder=(p-origin)%length;return origin+(remainder<0?remainder+length:remainder);}
kernel void filter_convolve_matrix_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::write> target [[texture(3)]],
    const device float* weights [[buffer(6)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint center=pack_pixel(source.read(xy)),divisor_bits=as_type<uint>(config.amount)&0x7fffffffu;
    if(!config.kernel_columns || !config.kernel_rows || !divisor_bits) {target.write(unpack_pixel(center),xy);return;}
    int2 lower=int2(config.region_x0,config.region_y0),size=int2(config.region_width,config.region_height),upper=lower+size;
    float4 sum=0;
    for(uint y=0;y<config.kernel_rows;++y) for(uint x=0;x<config.kernel_columns;++x) {
        uint index=config.kernel_offset+(config.kernel_rows-y-1)*config.kernel_columns+config.kernel_columns-x-1;
        int2 position=int2(xy)+int2(x,y)-int2(config.kernel_target_x,config.kernel_target_y);
        if(config.kernel_edge_mode==1) position=clamp(position,lower,upper-1);
        else if(config.kernel_edge_mode==2) position=int2(wrap_convolve(position.x,lower.x,size.x),wrap_convolve(position.y,lower.y,size.y));
        uint pixel=all(position>=lower) && all(position<upper)?pack_pixel(source.read(uint2(position))):0;
        if(config.linear_rgb==1) pixel=filter_premul_srgb_to_linear(pixel);
        float4 value=unpack_pixel(pixel);
        if(config.kernel_preserve_alpha==1) value.rgb=value.a==0.0f?float3(0):value.rgb/value.a;
        value.a=float(pixel>>24);
        sum=fma(value,weights[index],sum);
    }
    float3 color_quotient;float alpha_quotient,bias=config.rect_x0;
    if(divisor_bits<0x00800000u) {
        float divisor=float(divisor_bits)*as_type<float>(0x01000000u);
        if(as_type<uint>(config.amount)>>31) divisor=-divisor;
        float3 numerator(exponent_scale(sum.r,true),exponent_scale(sum.g,true),exponent_scale(sum.b,true));
        color_quotient=numerator/divisor;
        alpha_quotient=exponent_scale(sum.a,true)/(divisor*255.0f);
    } else if(divisor_bits>0x7e800000u) {
        float inverse=1.0f/exponent_scale(config.amount,false);
        float3 numerator(exponent_scale(sum.r,false),exponent_scale(sum.g,false),exponent_scale(sum.b,false));
        color_quotient=numerator*inverse;
        alpha_quotient=exponent_scale(sum.a,false)*inverse*(1.0f/255.0f);
    } else {
        float inverse=1.0f/config.amount;
        color_quotient=sum.rgb*inverse;
        alpha_quotient=(sum.a*inverse)*(1.0f/255.0f);
    }
    float unbounded_alpha=config.kernel_preserve_alpha==1?float(center>>24)*(1.0f/255.0f):alpha_quotient+bias;
    float alpha=clamp(unbounded_alpha,0.0f,1.0f);
    float3 color=color_quotient+bias*unbounded_alpha;
    color=config.kernel_preserve_alpha==1?clamp(color,0.0f,1.0f)*alpha:clamp(color,0.0f,alpha);
    uint result=pack_pixel(float4(color,alpha));
    target.write(unpack_pixel(config.linear_rgb==1?filter_premul_linear_to_srgb(result):result),xy);
}
