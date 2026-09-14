#include "../constants.hlsli"
#include "../fine/config.hlsli"
#include "../shared/brush/constants.hlsli"
#include "../shared/brush/data.hlsli"
#include "../shared/brush/linear.hlsli"
#include "../shared/brush/radial.hlsli"
#include "../shared/brush/sweep.hlsli"
#include "../shared/brush/four_corner.hlsli"
ConstantBuffer<FineConfig> config : register(b0);
struct RequestConfig { uint count; uint pad0; uint pad1; uint pad2; };
ConstantBuffer<RequestConfig> request_config : register(b11);
ByteAddressBuffer paint : register(t3);
ByteAddressBuffer requests : register(t9);
RWByteAddressBuffer output : register(u10);
[numthreads(FINE_WORKGROUP_SIZE,1,1)]
void gradient_words(uint3 id : SV_DispatchThreadID) {
    // Physical buffer allocation may include padded records; use the logical count.
    if(id.x>=request_config.count) return;
    uint3 record=requests.Load3(id.x*16u);
    uint data_base=record.x,brush_base=config.paint_brush_base;
    float x=asfloat(record.y),y=asfloat(record.z);
    uint kind=brush_word(paint,brush_base,data_base),extend=brush_word(paint,brush_base,data_base+1u);
    uint payload=data_base+brush_word(paint,brush_base,data_base+2u),len=brush_word(paint,brush_base,data_base+3u);
    uint base=data_base+BRUSH_HEADER_WORDS;
    uint color=brush_word(paint,brush_base,data_base+BRUSH_COLOR_WORD);
    if(kind==BRUSH_LINEAR) color=sample_linear(paint,brush_base,x,y,base,extend,payload,len);
    else if(kind==BRUSH_RADIAL) color=sample_radial(paint,brush_base,x,y,base,extend,payload,len);
    else if(kind==BRUSH_SWEEP) color=sample_sweep(paint,brush_base,x,y,base,extend,payload,len);
    else if(kind==BRUSH_FOUR_CORNER) color=sample_four_corner(paint,brush_base,x,y,base,payload);
    output.Store(id.x*4u,color);
}
