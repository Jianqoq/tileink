#ifndef TILEINK_TURBULENCE_NOISE_HLSLI
#define TILEINK_TURBULENCE_NOISE_HLSLI
#include "turbulence_constants.hlsli"
#include "../shared/integer.hlsli"
float turbulence_gradient_dot(ByteAddressBuffer gradients,uint table,uint channel,uint selector,float2 relative_position) {
    uint index=table*TURBULENCE_GRADIENT_LEN+(channel*TURBULENCE_TABLE_LEN+selector)*TURBULENCE_GRADIENT_COMPONENTS;
    float2 gradient=asfloat(gradients.Load2(index*4u));
    return mad(gradient.x,relative_position.x,gradient.y*relative_position.y);
}
float turbulence_curve(float t) {return t*t*(3.0-2.0*t);}
float turbulence_interpolate(float a,float b,float t) {return mad(b-a,t,a);}
// Full Euclidean wrapping also covers one-cell periods and negative lattice coordinates.
int turbulence_wrap(int value,int wrap,int period) {
    if(period==0) return value;
    int base=wrap-period;
    uint a=euclidean_remainder_i32(value,uint(period));
    uint b=euclidean_remainder_i32(base,uint(period));
    uint relative=a>=b ? a-b : uint(period)-(b-a);
    return base+int(relative);
}
float turbulence_noise(ByteAddressBuffer selectors,ByteAddressBuffer gradients,uint table,uint channel,float2 position,bool stitch,int2 wrap,int2 extent) {
    float2 translated=position+TURBULENCE_COORDINATE_OFFSET;
    int2 lower=int2(floor(translated)),upper=lower+1;
    float2 relative_position=translated-float2(lower);
    if(stitch) {
        lower=int2(turbulence_wrap(lower.x,wrap.x,extent.x),turbulence_wrap(lower.y,wrap.y,extent.y));
        upper=int2(turbulence_wrap(upper.x,wrap.x,extent.x),turbulence_wrap(upper.y,wrap.y,extent.y));
    }
    uint2 lo=uint2(lower & int(TURBULENCE_LATTICE_SIZE-1u));
    uint2 hi=uint2(upper & int(TURBULENCE_LATTICE_SIZE-1u));
    uint offset=table*TURBULENCE_TABLE_LEN;
    uint ix=selectors.Load((offset+lo.x)*4u),jx=selectors.Load((offset+hi.x)*4u);
    uint b00=selectors.Load((offset+ix+lo.y)*4u),b10=selectors.Load((offset+jx+lo.y)*4u);
    uint b01=selectors.Load((offset+ix+hi.y)*4u),b11=selectors.Load((offset+jx+hi.y)*4u);
    float sx=turbulence_curve(relative_position.x),sy=turbulence_curve(relative_position.y);
    float a=turbulence_interpolate(turbulence_gradient_dot(gradients,table,channel,b00,relative_position),turbulence_gradient_dot(gradients,table,channel,b10,relative_position-float2(1,0)),sx);
    float b=turbulence_interpolate(turbulence_gradient_dot(gradients,table,channel,b01,relative_position-float2(0,1)),turbulence_gradient_dot(gradients,table,channel,b11,relative_position-1.0),sx);
    return turbulence_interpolate(a,b,sy);
}
float turbulence_stitch_frequency(float frequency,float tile_size) {
    if(frequency<=0.0 || tile_size<=0.0) return 0.0;
    float low=floor(tile_size*frequency)/tile_size,high=ceil(tile_size*frequency)/tile_size;
    return low!=0.0 && frequency/low<high/frequency ? low : high;
}
#endif
