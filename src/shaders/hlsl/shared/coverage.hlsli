#ifndef TILEINK_HLSL_SHARED_COVERAGE_HLSLI_INCLUDED
#define TILEINK_HLSL_SHARED_COVERAGE_HLSLI_INCLUDED
#include "pixel.hlsli"
#include "../constants.hlsli"
#include "../scene_records.hlsli"

float4 segment_row_parts(float p0x, float p0y, float p1x, float p1y, float y_edge, uint y) {
    float delta_x=p1x-p0x, delta_y=p1y-p0y;
    float row_y=float(y), local_y=p0y-row_y;
    // Direct endpoint evaluation avoids cancellation at half-alpha boundaries.
    float y0=clamp(local_y,0.0,1.0), y1=clamp(p1y-row_y,0.0,1.0);
    float dy=y0-y1;
    float row_edge=signum_f32(delta_x)*clamp(row_y-y_edge+1.0,0.0,1.0);
    if (dy==0.0) return float4(row_edge,dy,0.0,0.0);
    // Anchor at the nearer endpoint and fuse each intersection, matching the
    // production coverage contract at half-channel quantization boundaries.
    float slope=delta_x/delta_y;
    bool nearer_p0=abs(local_y-0.5)<=abs(p1y-row_y-0.5);
    float anchor_x=nearer_p0?p0x:p1x;
    float anchor_y=nearer_p0?local_y:p1y-row_y;
    float sx0=mad(y0-anchor_y,slope,anchor_x);
    float sx1=mad(y1-anchor_y,slope,anchor_x);
    return float4(row_edge,dy,min(sx0,sx1),max(sx0,sx1));
}
float segment_area_at(float xmin_abs,float xmax_abs,uint x) {
    float xmin=xmin_abs-float(x), xmax=xmax_abs-float(x);
    float width=xmax-xmin;
    if (width==0.0) return clamp(1.0-xmin,0.0,1.0);
    if (xmin>=0.0 && xmax<=1.0) return mad(-0.5,xmin+xmax,1.0);
    float left=clamp(xmin,0.0,1.0), right=clamp(xmax,0.0,1.0);
    float full=clamp(-xmin,0.0,width);
    return mad(right-left,mad(-0.5,left+right,1.0),full)/width;
}
uint fill_alpha_at(ByteAddressBuffer segments,int backdrop,uint fill_rule,uint segment_start,uint segment_end,uint x,uint y) {
    // Preserve row-sweep accumulation order; do not reassociate the three sums.
    float base=float(backdrop), running=0.0, partial=0.0;
    for (uint segment_ix=segment_start;segment_ix<segment_end;++segment_ix) {
        uint address=segment_ix*LINE_SEGMENT_STRIDE;
        float4 points=asfloat(segments.Load4(address));
        float4 parts=segment_row_parts(points.x,points.y,points.z,points.w,asfloat(segments.Load(address+LINE_SEGMENT_Y_EDGE)),y);
        base+=parts.x;
        if (parts.y!=0.0) {
            int full_start=clamp(int(ceil(parts.w)),0,int(TILE_SIZE));
            if (full_start<int(TILE_SIZE) && int(x)>=full_start) running+=parts.y;
            int partial_start=clamp(int(floor(parts.z)),0,int(TILE_SIZE));
            int partial_end=clamp(int(ceil(parts.w)),0,int(TILE_SIZE));
            if (int(x)>=partial_start && int(x)<partial_end) partial+=segment_area_at(parts.z,parts.w,x)*parts.y;
        }
    }
    return coverage_to_alpha(base+running+partial,fill_rule);
}
#endif // TILEINK_HLSL_SHARED_COVERAGE_HLSLI_INCLUDED
