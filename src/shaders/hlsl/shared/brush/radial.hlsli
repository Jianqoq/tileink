#ifndef TILEINK_HLSL_BRUSH_RADIAL_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_RADIAL_HLSLI_INCLUDED
#include "data.hlsli"
#include "ramp.hlsli"
static const float RADIAL_QUADRATIC_EPSILON=0.000001;
uint sample_radial(ByteAddressBuffer paint,uint brush_base,float x,float y,uint base,uint extend,uint payload_offset,uint payload_len) {
    float tx=mad(brush_param(paint,brush_base,base,6u),x,mad(brush_param(paint,brush_base,base,8u),y,brush_param(paint,brush_base,base,10u)));
    float ty=mad(brush_param(paint,brush_base,base,7u),x,mad(brush_param(paint,brush_base,base,9u),y,brush_param(paint,brush_base,base,11u)));
    float sx=brush_param(paint,brush_base,base,0u),sy=brush_param(paint,brush_base,base,1u);
    float ex=brush_param(paint,brush_base,base,2u),ey=brush_param(paint,brush_base,base,3u);
    float start_radius=brush_param(paint,brush_base,base,4u),end_radius=brush_param(paint,brush_base,base,5u);
    float qx=tx-sx,qy=ty-sy,dcx=ex-sx,dcy=ey-sy,dr=end_radius-start_radius;
    float a=mad(dcx,dcx,mad(dcy,dcy,-dr*dr));
    float b=-2.0*mad(qx,dcx,mad(qy,dcy,start_radius*dr));
    float c=mad(qx,qx,mad(qy,qy,-start_radius*start_radius));
    bool has_t=false;float t=0.0;
    if (abs(a)<=RADIAL_QUADRATIC_EPSILON) {
        if (abs(b)>RADIAL_QUADRATIC_EPSILON) {
            float candidate=-c/b;
            if (mad(candidate,dr,start_radius)>=0.0) {has_t=true;t=candidate;}
        }
    } else {
        float discriminant=mad(b,b,-4.0*a*c);
        if (discriminant>=0.0) {
            float root=sqrt(discriminant);
            float t0=(-b-root)/(2.0*a),t1=(-b+root)/(2.0*a);
            bool valid0=mad(t0,dr,start_radius)>=0.0,valid1=mad(t1,dr,start_radius)>=0.0;
            if (valid0) {has_t=true;t=valid1?max(t0,t1):t0;}
            else if(valid1) {has_t=true;t=t1;}
        }
    }
    return has_t?sample_ramp(paint,brush_base,payload_offset,payload_len,t,extend):0u;
}
#endif // TILEINK_HLSL_BRUSH_RADIAL_HLSLI_INCLUDED
