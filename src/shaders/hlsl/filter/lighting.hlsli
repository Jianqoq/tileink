#ifndef TILEINK_FILTER_LIGHTING_HLSLI
#define TILEINK_FILTER_LIGHTING_HLSLI
#include "config.hlsli"
#include "../shared/pixel.hlsli"
static const float LIGHTING_DEGREES_TO_RADIANS=0.017453292;
static const float LIGHTING_DIRECTION_EPSILON=0.000001;
// A zero exponent denotes unit intensity even at zero focus; pow(0,0) is not portable.
float lighting_power(float base,float exponent) {
    if (exponent<=0.0) return 1.0;
    return pow(base,exponent);
}
float lighting_dot3(float3 a,float3 b) { return mad(a.x,b.x,mad(a.y,b.y,mad(a.z,b.z,0.0))); }
float lighting_alpha_byte(Texture2D<float4> source,uint2 xy) {
    return float(unorm_to_rgba8(source.Load(int3(xy,0)))>>24u);
}
// Sobel-like finite differences use one-sided scaling at primitive boundaries.
// Accumulate byte differences before normalization, preserving the production order.
float lighting_gradient(ConstantBuffer<FilterConfig> config,Texture2D<float4> source,uint2 xy,bool horizontal) {
    uint2 lower=uint2(config.region_x0,config.region_y0), size=uint2(config.region_width,config.region_height);
    uint length=horizontal ? size.x : size.y;
    if (length<2u) return 0.0;
    uint2 upper=lower+size;
    float weighted_diff=0.0,weight_sum=0.0;
    for (int offset=-1;offset<=1;++offset) {
        float weight=offset==0 ? 2.0 : 1.0;
        int cross_position=int(horizontal ? xy.y : xy.x)+offset;
        int cross_min=int(horizontal ? lower.y : lower.x), cross_max=int(horizontal ? upper.y : upper.x);
        if (cross_position<cross_min || cross_position>=cross_max) continue;
        uint position=horizontal ? xy.x : xy.y;
        uint start=horizontal ? lower.x : lower.y, end=horizontal ? upper.x : upper.y;
        uint before=position>start ? position-1u : position;
        uint after=position<end-1u ? position+1u : position;
        uint2 a=horizontal ? uint2(before,uint(cross_position)) : uint2(uint(cross_position),before);
        uint2 b=horizontal ? uint2(after,uint(cross_position)) : uint2(uint(cross_position),after);
        weighted_diff+=weight*(lighting_alpha_byte(source,b)-lighting_alpha_byte(source,a));
        weight_sum+=weight;
    }
    uint pos=horizontal ? xy.x : xy.y, start=horizontal ? lower.x : lower.y;
    float scale=pos==start || pos==start+length-1u ? 2.0 : 1.0;
    return weighted_diff*scale/(255.0*weight_sum);
}
uint filter_lighting_pixel(ConstantBuffer<FilterConfig> config,Texture2D<float4> source,uint2 xy) {
    uint no_light=config.lighting_output_kind==0u ? 0xff000000u : 0u;
    float alpha=lighting_alpha_byte(source,xy)/255.0;
    float z=alpha*config.surface_scale;
    float dx=lighting_gradient(config,source,xy,true)*config.surface_scale;
    float dy=lighting_gradient(config,source,xy,false)*config.surface_scale;
    float3 normal=float3(-dx,-dy,1.0);
    float normal_len=sqrt(lighting_dot3(normal,normal));
    float world_x=float(config.surface_origin_x)+float(xy.x)+0.5;
    float world_y=float(config.surface_origin_y)+float(xy.y)+0.5;
    float3 light=float3(config.light_p0-world_x,config.light_p1-world_y,config.light_p2-z);
    float attenuation=1.0;
    if (config.light_kind==0u) {
        float azimuth=config.light_p0*LIGHTING_DEGREES_TO_RADIANS;
        float elevation=config.light_p1*LIGHTING_DEGREES_TO_RADIANS;
        light=float3(cos(azimuth)*cos(elevation),sin(azimuth)*cos(elevation),sin(elevation));
    } else {
        float len=sqrt(lighting_dot3(light,light));
        if (len<=LIGHTING_DIRECTION_EPSILON) return no_light;
        light/=len;
        if (config.light_kind==2u) {
            float3 spot=float3(config.light_p3-config.light_p0,config.light_p4-config.light_p1,config.light_p5-config.light_p2);
            float spot_len=sqrt(lighting_dot3(spot,spot));
            if (spot_len<=LIGHTING_DIRECTION_EPSILON) return no_light;
            spot/=spot_len;
            float focus=-lighting_dot3(light,spot);
            if (focus<0.0) return no_light;
            if (config.light_p7>=0.0 && focus<cos(config.light_p7*LIGHTING_DEGREES_TO_RADIANS)) return no_light;
            attenuation=lighting_power(focus,config.light_p6);
        }
    }
    float3 color=float3(config.light_r,config.light_g,config.light_b);
    if (config.lighting_output_kind==0u) {
        float amount=config.light_constant*attenuation*max(lighting_dot3(normal/normal_len,light),0.0);
        color=clamp(color*amount,0.0,1.0);
        return pack_premul_rgba8(color.r,color.g,color.b,1.0);
    }
    float3 half_vector=float3(light.xy,light.z+1.0);
    float half_len=sqrt(lighting_dot3(half_vector,half_vector));
    if (half_len<=LIGHTING_DIRECTION_EPSILON) return no_light;
    float cosine=max(lighting_dot3(normal,half_vector)/(normal_len*half_len),0.0);
    float amount=config.light_constant*attenuation*lighting_power(cosine,config.specular_exponent);
    color=clamp(color*amount,0.0,1.0);
    return pack_premul_rgba8(color.r,color.g,color.b,max(max(color.r,color.g),color.b));
}
#endif
