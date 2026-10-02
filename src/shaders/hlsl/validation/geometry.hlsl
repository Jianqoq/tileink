#include "../constants.hlsli"
#include "../shared/coverage.hlsli"
#include "../shared/pattern_transform.hlsli"
struct MathConfig { uint count; uint pad0; uint pad1; uint pad2; };
ConstantBuffer<MathConfig> config : register(b0);
ByteAddressBuffer source : register(t1);
RWByteAddressBuffer destination : register(u2);
[numthreads(FINE_WORKGROUP_SIZE,1,1)]
void geometry_math_words(uint3 id : SV_DispatchThreadID) {
    if (id.x>=config.count) return;
    uint base=id.x*32u;
    float4 points=asfloat(source.Load4(base));
    float edge=asfloat(source.Load(base+16u));
    uint y=source.Load(base+20u), x=source.Load(base+24u), rule=source.Load(base+28u);
    float4 parts=segment_row_parts(points.x,points.y,points.z,points.w,edge,y);
    float area=segment_area_at(parts.z,parts.w,x);
    float coordinate=pattern_transform_component(points.x,points.y,edge,points.z,points.w);
    destination.Store4(id.x*16u,uint4(coverage_to_alpha(parts.x+area*parts.y,rule),coverage_to_u8(area),asuint(int(floor(coordinate))),coverage_to_alpha(parts.y,rule)));
}

ByteAddressBuffer segments : register(t3);
[numthreads(FINE_WORKGROUP_SIZE,1,1)]
void fill_coverage_words(uint3 id : SV_DispatchThreadID) {
    if (id.x>=config.count) return;
    uint base=id.x*32u;
    uint4 request=source.Load4(base);
    uint2 tail=source.Load2(base+16u);
    destination.Store(id.x*4u,fill_alpha_at(segments,asint(request.z),tail.y,request.x,request.y,request.w,tail.x));
}
