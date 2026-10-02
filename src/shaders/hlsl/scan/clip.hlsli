#ifndef TILEINK_HLSL_SCAN_CLIP_HLSLI_INCLUDED
#define TILEINK_HLSL_SCAN_CLIP_HLSLI_INCLUDED

#include "geometry.hlsli"
#include "../constants.hlsli"
#include "../scene_records.hlsli"
// Explicit mad corresponds to the reference shader's intersection fma. HLSL
// permits device-specific mad implementations, so exact results remain gated by
// the same-physical-GPU four-route tests, not assumed from the intrinsic name.
float x_at_y(float4 points,float y) {
    return mad(points.z-points.x,(y-points.y)/(points.w-points.y),points.x);
}
float y_at_x(float4 points,float x) {
    return mad(points.w-points.y,(x-points.x)/(points.z-points.x),points.y);
}
float clip_y_at_x(float4 points,float x,float tile_min_y,float tile_max_y) {
    float y=y_at_x(points,x);
    if(y<=tile_min_y+SCAN_EPSILON) {
        float top_x=x_at_y(points,tile_min_y);
        if(abs(top_x-x)>TILE_CLIP_NUDGE) return tile_min_y;
        return tile_min_y+TILE_CLIP_NUDGE;
    }
    return clamp(y,tile_min_y+TILE_CLIP_NUDGE,tile_max_y);
}

void write_clipped_segment(RWByteAddressBuffer output_segments, uint dst,ScanTraversal scan,uint sub_index,float z,int2 tile) {
    float tile_size=float(TILE_SIZE);
    float2 tile_min=float2(tile)*tile_size;
    float2 tile_max=tile_min+tile_size;
    float4 points=scan.points;
    if(sub_index>0u) {
        float previous=floor(scan.a*(float(sub_index)-1.0)+scan.b);
        if(z==previous) {
            float x=x_at_y(scan.points,tile_min.y);
            x=clamp(x,tile_min.x+TILE_CLIP_NUDGE,tile_max.x);
            points.xy=float2(x,tile_min.y);
        } else {
            float x_clip=tile_max.x;if(scan.sign>0.0) x_clip=tile_min.x;
            points.xy=float2(x_clip,clip_y_at_x(scan.points,x_clip,tile_min.y,tile_max.y));
        }
    }
    if(sub_index<scan.count-1u) {
        float next=floor(scan.a*(float(sub_index)+1.0)+scan.b);
        if(z==next) {
            float x=x_at_y(scan.points,tile_max.y);
            x=clamp(x,tile_min.x+TILE_CLIP_NUDGE,tile_max.x);
            points.zw=float2(x,tile_max.y);
        } else {
            float x_clip=tile_min.x;if(scan.sign>0.0) x_clip=tile_max.x;
            points.zw=float2(x_clip,clip_y_at_x(scan.points,x_clip,tile_min.y,tile_max.y));
        }
    }
    float edge=1000000000.0;
    float p0x=clamp(points.x-tile_min.x,0.0,tile_size);
    float p0y=clamp(points.y-tile_min.y,0.0,tile_size);
    float p1x=clamp(points.z-tile_min.x,0.0,tile_size);
    float p1y=clamp(points.w-tile_min.y,0.0,tile_size);
    if(p0x<=SCAN_EPSILON) p0x=0.0;else if(tile_size-p0x<=SCAN_EPSILON) p0x=tile_size;
    if(p0y<=SCAN_EPSILON) p0y=0.0;else if(tile_size-p0y<=SCAN_EPSILON) p0y=tile_size;
    if(p1x<=SCAN_EPSILON) p1x=0.0;else if(tile_size-p1x<=SCAN_EPSILON) p1x=tile_size;
    if(p1y<=SCAN_EPSILON) p1y=0.0;else if(tile_size-p1y<=SCAN_EPSILON) p1y=tile_size;
    if(p0x==0.0) {
        if(p1x==0.0) {
            p0x=SCAN_EPSILON;
            if(p0y==0.0) {p1x=SCAN_EPSILON;p1y=tile_size;}
            else {p1x=2.0*SCAN_EPSILON;p1y=p0y;}
        } else if(p0y==0.0) p0x=SCAN_EPSILON;
        else edge=p0y;
    } else if(p1x==0.0) {if(p1y==0.0) p1x=SCAN_EPSILON;else edge=p1y;}
    if(floor(p0x)==p0x && p0x!=0.0) p0x-=SCAN_EPSILON;
    if(floor(p1x)==p1x && p1x!=0.0) p1x-=SCAN_EPSILON;
    if(!scan.down) {float x=p0x;float y=p0y;p0x=p1x;p0y=p1y;p1x=x;p1y=y;}
    output_segments.Store4(dst*LINE_SEGMENT_STRIDE,asuint(float4(p0x,p0y,p1x,p1y)));
    output_segments.Store(dst*LINE_SEGMENT_STRIDE+LINE_SEGMENT_Y_EDGE,asuint(edge));
}

#endif // TILEINK_HLSL_SCAN_CLIP_HLSLI_INCLUDED
