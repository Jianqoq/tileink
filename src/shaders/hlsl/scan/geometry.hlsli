#ifndef TILEINK_HLSL_SCAN_GEOMETRY_HLSLI_INCLUDED
#define TILEINK_HLSL_SCAN_GEOMETRY_HLSLI_INCLUDED

#include "../constants.hlsli"
#include "../scene_records.hlsli"
// Shared DDA traversal for count and emit; arithmetic order follows the renderer
// geometry contract. Top-touch tolerance and tile snapping are distinct policies.
static const float SCAN_EPSILON=1.0e-6;
static const float TOP_TOUCH_EPSILON=1.0e-12;
static const float TILE_CLIP_NUDGE=1.0e-3;

struct ScanTraversal {
    uint4 bbox;
    uint data_offset;
    float4 points;
    bool down;
    uint count;
    float a,b,sign,x0,y0;
    uint imin,imax;
    int ymin,ymax;
};
bool is_tile_boundary_y(float value) {return abs(value-floor(value))<=TOP_TOUCH_EPSILON;}
int ceil_tile_boundary_y(float value) {if(is_tile_boundary_y(value)) return int(floor(value));return int(ceil(value));}
uint span(float a,float b) {float hi=ceil(a);if(b>a) hi=ceil(b);float lo=floor(a);if(b<a) lo=floor(b);float value=hi-lo;if(value<1.0) value=1.0;return uint(value);}

bool scan_geometry(ByteAddressBuffer line_buffer, ByteAddressBuffer path_buffer, uint line_index, out ScanTraversal scan) {
    scan=(ScanTraversal)0;
    uint line_base=line_index*LINE_STRIDE;
    uint path_id=line_buffer.Load(line_base);
    uint path_bytes;path_buffer.GetDimensions(path_bytes);
    if(path_id>=path_bytes/PATH_RECORD_STRIDE) return false;
    uint path_base=path_id*PATH_RECORD_STRIDE;
    scan.bbox=path_buffer.Load4(path_base+PATH_BBOX);
    scan.data_offset=path_buffer.Load(path_base+PATH_DATA_OFFSET);
    if(scan.bbox.z-scan.bbox.x==0u || scan.bbox.y>=scan.bbox.w) return false;
    float4 affine_linear=asfloat(path_buffer.Load4(path_base+PATH_TRANSFORM));
    float2 translation=asfloat(path_buffer.Load2(path_base+PATH_TRANSFORM+AFFINE_TRANSLATION));
    float2 local0=asfloat(line_buffer.Load2(line_base+LINE_P0));
    float2 local1=asfloat(line_buffer.Load2(line_base+LINE_P1));
    float2 p0=float2(affine_linear.x*local0.x+affine_linear.z*local0.y+translation.x,affine_linear.y*local0.x+affine_linear.w*local0.y+translation.y);
    float2 p1=float2(affine_linear.x*local1.x+affine_linear.z*local1.y+translation.x,affine_linear.y*local1.x+affine_linear.w*local1.y+translation.y);
    scan.down=p1.y>=p0.y;
    scan.points=float4(p0,p1);
    if(!scan.down) scan.points=float4(p1,p0);
    float tile_scale=1.0/float(TILE_SIZE);
    float4 s=scan.points*tile_scale;
    uint count_x=span(s.x,s.z)-1u;
    scan.count=count_x+span(s.y,s.w);
    float dx=abs(s.z-s.x);
    float dy=s.w-s.y;
    if(dx+dy==0.0 || (dy==0.0 && floor(s.y)==s.y)) return false;
    float reciprocal=1.0/(dx+dy);
    scan.a=dx*reciprocal;
    bool positive=s.z>=s.x;
    scan.sign=-1.0;if(positive) scan.sign=1.0;
    float xt0=floor(s.x*scan.sign);
    float c=s.x*scan.sign-xt0;
    scan.y0=floor(s.y);
    float ytop=scan.y0+1.0;if(s.y==s.w) ytop=ceil(s.y);
    scan.b=min((dy*c+dx*(ytop-s.y))*reciprocal,0.99999994);
    float error=floor(scan.a*(float(scan.count)-1.0)+scan.b)-float(count_x);
    if(error!=0.0) {if(error>0.0) scan.a-=0.0000002;else scan.a+=0.0000002;}
    scan.x0=xt0*scan.sign-1.0;if(positive) scan.x0=xt0*scan.sign;
    float xmin=min(s.x,s.z);
    if(s.y>=float(scan.bbox.w) || s.w<=float(scan.bbox.y)+TOP_TOUCH_EPSILON || xmin>=float(scan.bbox.z)) return false;
    scan.imin=0u;
    if(s.y<float(scan.bbox.y)) {
        float first=round((float(scan.bbox.y)-scan.y0+scan.b-scan.a)/(1.0-scan.a))-1.0;
        if(scan.y0+first-floor(scan.a*first+scan.b)<float(scan.bbox.y)) first+=1.0;
        scan.imin=uint(first);
    }
    scan.imax=scan.count;
    if(s.w>float(scan.bbox.w)) {
        float last=round((float(scan.bbox.w)-scan.y0+scan.b-scan.a)/(1.0-scan.a))-1.0;
        if(scan.y0+last-floor(scan.a*last+scan.b)<float(scan.bbox.w)) last+=1.0;
        scan.imax=uint(last);
    }
    scan.ymin=0;scan.ymax=0;
    if(max(s.x,s.z)<float(scan.bbox.x)) {
        scan.ymin=ceil_tile_boundary_y(s.y);scan.ymax=ceil_tile_boundary_y(s.w);
        scan.imax=scan.imin;
    } else {
        float fudge=1.0;if(positive) fudge=0.0;
        if(xmin<float(scan.bbox.x)) {
            float cut=round((scan.sign*(float(scan.bbox.x)-scan.x0)-scan.b+fudge)/scan.a);
            if((scan.x0+scan.sign*floor(scan.a*cut+scan.b)<float(scan.bbox.x))==positive) cut+=1.0;
            int ynext=int(scan.y0+cut-floor(scan.a*cut+scan.b)+1.0);
            if(positive) {
                if(uint(cut)>scan.imin) {
                    float ystart=scan.y0+1.0;if(is_tile_boundary_y(s.y)) ystart=scan.y0;
                    scan.ymin=int(ystart);scan.ymax=ynext;scan.imin=uint(cut);
                }
            } else if(uint(cut)<scan.imax) {scan.ymin=ynext;scan.ymax=ceil_tile_boundary_y(s.w);scan.imax=uint(cut);}
        }
        if(max(s.x,s.z)>float(scan.bbox.z)) {
            float cut=round((scan.sign*(float(scan.bbox.z)-scan.x0)-scan.b+fudge)/scan.a);
            if((scan.x0+scan.sign*floor(scan.a*cut+scan.b)<float(scan.bbox.z))==positive) cut+=1.0;
            if(positive) scan.imax=min(scan.imax,uint(cut));else scan.imin=max(scan.imin,uint(cut));
        }
    }
    scan.imax=max(scan.imin,scan.imax);
    scan.ymin=max(scan.ymin,int(scan.bbox.y));scan.ymax=min(scan.ymax,int(scan.bbox.w));
    return true;
}

int2 scan_tile(ScanTraversal scan,uint index,float z) {
    return int2(int(scan.x0+scan.sign*z),int(scan.y0+float(index)-z));
}
bool scan_tile_inside(ScanTraversal scan,int2 tile) {
    return tile.y>=int(scan.bbox.y) && tile.y<int(scan.bbox.w) && tile.x>=int(scan.bbox.x) && tile.x<int(scan.bbox.z);
}
uint scan_tile_offset(ScanTraversal scan,int2 tile) {
    return scan.data_offset+(uint(tile.y)-scan.bbox.y)*(scan.bbox.z-scan.bbox.x)+uint(tile.x)-scan.bbox.x;
}

#endif // TILEINK_HLSL_SCAN_GEOMETRY_HLSLI_INCLUDED
