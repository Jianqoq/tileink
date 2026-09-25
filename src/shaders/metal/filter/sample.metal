// Bilinear interpolation uses logical extents, independent of pooled allocation.
float4 sample_premul(texture2d<float,access::read> source,uint2 extent,float2 position) {
    position=clamp(position,0.0f,float2(extent-1));
    uint2 a=uint2(floor(position)),b=min(a+1,extent-1);
    float2 phase=position-float2(a);
    float4 top_left=source.read(a),bottom_left=source.read(uint2(a.x,b.y));
    float4 top=fma(source.read(uint2(b.x,a.y))-top_left,phase.x,top_left);
    float4 bottom=fma(source.read(b)-bottom_left,phase.x,bottom_left);
    return fma(bottom-top,phase.y,top);
}
