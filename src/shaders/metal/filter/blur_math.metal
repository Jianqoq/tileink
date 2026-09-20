#include "sample.metal"
int4 blur_bounds(constant FilterConfig& config) {
    bool x=config.source_x1>config.source_x0,y=config.source_y1>config.source_y0;
    return int4(x?config.source_x0:config.region_x0,y?config.source_y0:config.region_y0,
        x?config.source_x1:config.region_x0+config.region_width,y?config.source_y1:config.region_y0+config.region_height);
}
bool blur_inside(int2 p,int4 bounds) {return all(p>=bounds.xy) && all(p<bounds.zw);}
uint blur_pack(float4 sum,float weight) {
    return weight>0?pack_bytes(uint4(clamp(fma(sum,1.0f/weight,0.0f)+0.5f,0.0f,255.0f))):0;
}
float4 blur_read(texture2d<float,access::read> source,int2 p) {return float4(byte_channels(pack_pixel(source.read(uint2(p)))));}
// Paired taps retain the shared recurrence and explicit FMA rounding boundaries.
uint blur_pixel(constant FilterConfig& config,texture2d<float,access::read> source,uint2 xy,float deviation) {
    int radius=int(max(ceil(deviation*3.0f),1.0f));
    float sigma=max(deviation,0.0001f),variance=2.0f*sigma*sigma;
    int2 axis=config.blur_axis==0?int2(1,0):int2(0,1),center=int2(xy);
    int4 bounds=blur_bounds(config);
    float4 accumulated=blur_inside(center,bounds)?blur_read(source,center):float4(0);
    float total=1,weight=exp(-1.0f/variance),decay=exp(-2.0f/variance),ratio=weight*decay;
    for(int distance=1;distance<=radius;distance+=2) {
        bool pair=distance+1<=radius;
        float second=weight*ratio,combined=weight+(pair?second:0.0f);
        total+=2.0f*combined;
        for(int side=1;side>=-1;side-=2) {
            int2 first=center+axis*(side>0?distance:-(distance+1));
            if(pair && blur_inside(first,bounds) && blur_inside(first+axis,bounds)) {
                float offset=float(side)*(float(distance)+second/combined);
                float4 sample=fma(sample_premul(source,uint2(config.width,config.height),float2(center)+float2(axis)*offset),255.0f,0.0f);
                accumulated=fma(sample,combined,accumulated);
            } else {
                int2 position=center+axis*(side*distance);
                if(blur_inside(position,bounds)) accumulated=fma(blur_read(source,position),weight,accumulated);
                position=center+axis*(side*(distance+1));
                if(pair && blur_inside(position,bounds)) accumulated=fma(blur_read(source,position),second,accumulated);
            }
        }
        weight=fma(second,fma(ratio,decay,0.0f),0.0f);
        ratio=fma(ratio,fma(decay,decay,0.0f),0.0f);
    }
    return blur_pack(accumulated,total);
}
