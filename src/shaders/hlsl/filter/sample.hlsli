#ifndef TILEINK_FILTER_SAMPLE_HLSLI
#define TILEINK_FILTER_SAMPLE_HLSLI
// Sample in logical texel space: physical pooled texture size never changes pixels.
float4 filter_sample_premul(Texture2D<float4> source, uint2 extent, float2 position) {
    int2 last=int2(extent)-1;
    position=clamp(position,0.0,float2(last));
    int2 first=int2(floor(position));
    int2 next=min(first+1,last);
    float2 phase=position-float2(first);
    float4 a=source.Load(int3(first,0));
    float4 b=source.Load(int3(next.x,first.y,0));
    float4 c=source.Load(int3(first.x,next.y,0));
    float4 d=source.Load(int3(next,0));
    float4 top=mad(b-a,phase.x,a);
    float4 bottom=mad(d-c,phase.x,c);
    return mad(bottom-top,phase.y,top);
}
#endif
