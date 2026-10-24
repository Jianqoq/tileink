#ifndef TILEINK_FILTER_CONVOLVE_HLSLI
#define TILEINK_FILTER_CONVOLVE_HLSLI
#include "config.hlsli"
#include "../shared/pixel.hlsli"
#include "color_space.hlsli"
#include "../shared/integer.hlsli"

// Signed remainder maps directly into the primitive region, even for kernels
// wider than that region. No repeated wrap loop or implicit robust load is used.
int convolve_wrap(int coordinate, int origin, int length) {
    return origin+int(euclidean_remainder_i32(coordinate-origin,uint(length)));
}
// Scale by 2^24 through exponent bits so fast-math cannot fold this into
// an overflowing reciprocal of a subnormal divisor.
float convolve_scale_24(float value) {
    uint bits=asuint(value);
    uint magnitude=bits&0x7fffffffu;
    uint exponent=magnitude>>23u;
    if (exponent==0u) {
        float scaled=float(magnitude)*asfloat(0x01000000u);
        return (bits&0x80000000u)!=0u ? -scaled : scaled;
    }
    if (exponent>=231u) return asfloat((bits&0x80000000u)|0x7f800000u);
    return asfloat(bits+(24u<<23u));
}
// Large divisors need the inverse normalization: their reciprocal can flush
// to zero although numerator/divisor has a representable, visible result.
float convolve_unscale_24(float value) {
    uint bits=asuint(value);
    uint exponent=(bits&0x7fffffffu)>>23u;
    if (exponent<=24u) return 0.0;
    return asfloat(bits-(24u<<23u));
}
uint filter_convolve_pixel(ConstantBuffer<FilterConfig> config, Texture2D<float4> source,
    ByteAddressBuffer kernels, uint2 xy) {
    uint center=unorm_to_rgba8(source.Load(int3(xy,0)));
    uint divisor_magnitude=asuint(config.amount)&0x7fffffffu;
    if (config.kernel_columns==0u || config.kernel_rows==0u || divisor_magnitude==0u) return center;
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
            if(config.linear_rgb==1u) pixel=filter_premul_srgb_to_linear(pixel);
            uint alpha=pixel>>24u;
            float4 value=rgba8_to_unorm(pixel);
            // preserveAlpha convolves straight colors; the default convolves
            // premultiplied colors together with alpha.
            if(config.kernel_preserve_alpha==1u) value.rgb=alpha==0u?0.0:value.rgb/value.a;
            value.a=float(alpha);
            sum=mad(value,weight,sum);
        }
    }
    // Keep alpha in its stored byte domain. Normalizing each tap and rescaling
    // after bias crosses half-byte boundaries differently between shader targets.
    float reciprocal=1.0/config.amount;
    float alpha_quotient;
    float3 color_quotient;
    if (divisor_magnitude<0x00800000u) {
        // Decode subnormal divisors before GPU flush-to-zero can erase them.
        // Scaling both sides by 2^24 puts every nonzero divisor in normal range.
        float scaled_divisor=float(divisor_magnitude)*asfloat(0x01000000u);
        if ((asuint(config.amount)&0x80000000u)!=0u) scaled_divisor=-scaled_divisor;
        color_quotient=float3(convolve_scale_24(sum.r),convolve_scale_24(sum.g),convolve_scale_24(sum.b))/scaled_divisor;
        alpha_quotient=convolve_scale_24(sum.a)/(scaled_divisor*255.0);
    } else if (divisor_magnitude>0x7e800000u) {
        float inverse=1.0/convolve_unscale_24(config.amount);
        float3 reduced=float3(convolve_unscale_24(sum.r),convolve_unscale_24(sum.g),convolve_unscale_24(sum.b));
        color_quotient=reduced*inverse;
        alpha_quotient=convolve_unscale_24(sum.a)*inverse*(1.0/255.0);
    } else {
        color_quotient=sum.rgb*reciprocal;
        alpha_quotient=(sum.a*reciprocal)*(1.0/255.0);
    }
    float unbounded_alpha=config.kernel_preserve_alpha==1u?float(center>>24u)*CHANNEL_SCALE:alpha_quotient+config.rect_x0;
    float alpha=saturate(unbounded_alpha);
    // SVG applies bias using the unbounded result alpha, then clamps the
    // premultiplied color to the bounded output alpha.
    float3 color=color_quotient+config.rect_x0*unbounded_alpha;
    color=config.kernel_preserve_alpha==1u?saturate(color)*alpha:clamp(color,0.0,alpha);
    uint result=pack_premul_rgba8(color.r,color.g,color.b,alpha);
    return config.linear_rgb==1u?filter_premul_linear_to_srgb(result):result;
}
#endif
