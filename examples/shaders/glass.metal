#include <metal_stdlib>
using namespace metal;
// Application-owned ABI and helpers. No code is injected by Tileink.
struct ShaderConfig {
    uint width,height,region_x0,region_y0,region_width,region_height;
    uint dispatch_width,pixel_count,compact_tiles,active_tile_count,tiles_width,tiles_height;
    float inverse_xx,inverse_yx,inverse_xy,inverse_yy,inverse_tx,inverse_ty;
    float sample_xx,sample_yx,sample_xy,sample_yy,sample_radius;
    uint uniform_words,output_x0,output_y0,output_width,output_height;
};
static_assert(sizeof(ShaderConfig)==112,"ShaderConfig ABI");
bool position(constant ShaderConfig& c,const device uint* tiles,uint3 group,uint lane,thread uint2& p) {
    uint index=(group.x+group.y*c.dispatch_width)*256+lane;
    if(index>=c.pixel_count) return false;
    if(c.compact_tiles) { uint tile=tiles[index/256];p=uint2(tile%c.tiles_width,tile/c.tiles_width)*16+uint2(index%16,(index/16)%16); }
    else p=uint2(c.region_x0+index%c.region_width,c.region_y0+index/c.region_width);
    return all(p<uint2(c.width,c.height)) && all(p>=uint2(c.region_x0,c.region_y0)) && all(p<uint2(c.region_x0+c.region_width,c.region_y0+c.region_height));
}
float2 logical(constant ShaderConfig& c,uint2 p) {
    float2 q=float2(p)+0.5f;
    return float2(c.inverse_xx*q.x+c.inverse_xy*q.y+c.inverse_tx,c.inverse_yx*q.x+c.inverse_yy*q.y+c.inverse_ty);
}
float4 border(texture2d<float,access::read> input,int2 p,uint2 extent) { return all(p>=0) && all(p<int2(extent))?input.read(uint2(p)):float4(0); }
float4 sample_input(texture2d<float,access::read> input,constant ShaderConfig& c,uint2 p,float2 offset) {
    offset=clamp(offset,-c.sample_radius,c.sample_radius);
    float2 q=float2(p)+float2(c.sample_xx*offset.x+c.sample_xy*offset.y,c.sample_yx*offset.x+c.sample_yy*offset.y);
    int2 a=int2(floor(q));float2 t=fract(q);uint2 extent(c.width,c.height);
    return mix(mix(border(input,a,extent),border(input,a+int2(1,0),extent),t.x),mix(border(input,a+int2(0,1),extent),border(input,a+int2(1,1),extent),t.x),t.y);
}
float4 output_color(float4 v) {
    if(!all(isfinite(v))) return float4(0);
    float a=clamp(v.a,0.0f,1.0f);v=clamp(v,0.0f,a);
    return floor(v*255.0f+0.5f)*(1.0f/255.0f);
}
kernel void glass(constant ShaderConfig& c [[buffer(0)]],texture2d<float,access::read> input [[texture(1)]],
    texture2d<float,access::write> output [[texture(3)]],const device uint* tiles [[buffer(8)]],const device uint* u [[buffer(10)]],uint3 group [[threadgroup_position_in_grid]],uint lane [[thread_index_in_threadgroup]]) {
    uint2 p;if(!position(c,tiles,group,lane,p)) return;
    float2 pos=logical(c,p);
    float2 relative=pos-as_type<float4>(uint4(u[0],u[1],u[2],u[3])).xy;
    float2 q=abs(relative)-as_type<float4>(uint4(u[4],u[5],u[6],u[7])).xy;
    float2 outside=max(q,0.0);
    float len=length(outside);
    float distance=len+min(max(q.x,q.y),0.0)-as_type<float4>(uint4(u[8],u[9],u[10],u[11])).x;
    float2 axis=step(q.yx,q)*(len<=1e-6?1.0:0.0);
    float2 normal=(outside+axis)*relative/max(abs(relative),float2(1e-6,1e-6));
    normal/=max(length(normal),1e-6);
    float rim=clamp(1.0+distance*as_type<float4>(uint4(u[12],u[13],u[14],u[15])).x,0.0,1.0);
    float2 shift=normal*rim*as_type<float4>(uint4(u[16],u[17],u[18],u[19])).xy;
    float4 sampled=sample_input(input,c,p,shift);
    if(u[28]!=0) { sampled.r=sample_input(input,c,p,shift*0.96).r;sampled.b=sample_input(input,c,p,shift*1.04).b; }
    float fresnel=rim*rim;fresnel*=fresnel;
    float4 color=sampled*as_type<float4>(uint4(u[20],u[21],u[22],u[23]))+fresnel*as_type<float4>(uint4(u[24],u[25],u[26],u[27]));
    float4 result=mix(input.read(p),color,clamp(-distance,0.0,1.0));
    output.write(output_color(result),p-uint2(c.output_x0,c.output_y0));
}
