#ifndef TILEINK_HLSL_BRUSH_MESH_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_MESH_HLSLI_INCLUDED
#include "data.hlsli"
#include "../pixel.hlsli"
// Same compiled spatial bins, stable quadratic inversion, and single final
// premultiplied quantization as Metal mesh.metal.
float mesh_cross(float2 a,float2 b) {return mad(a.x,b.y,-a.y*b.x);}
bool mesh_candidate(float u,float2 q,float2 b,float2 c,float2 d,out float2 uv) {
    uv=float2(0,0);
    float2 axis=c+d*u,rest=q-b*u;
    float v=abs(axis.x)>abs(axis.y)?rest.x/axis.x:rest.y/axis.y;
    if(!isfinite(u)||!isfinite(v)) return false;
    float2 du=b+d*v;
    float determinant=abs(mesh_cross(du,axis));
    if(!isfinite(determinant)||determinant<=0) return false;
    // Convert normalized-coordinate rounding into parameter error through the
    // inverse Jacobian; a fixed UV epsilon creates holes in thin valid quads.
    float2 tolerance=max(float2(0.00002,0.00002),float2(abs(axis.x)+abs(axis.y),abs(du.x)+abs(du.y))*(0.0000009536743164/determinant));
    if(any(float2(u,v)< -tolerance)||any(float2(u,v)>1.0+tolerance)) return false;
    uv=clamp(float2(u,v),0.0,1.0);return true;
}
bool mesh_inverse(float2 q,float2 b,float2 c,float2 d,out float2 uv) {
    uv=float2(0,0);
    float a=mesh_cross(b,d),coefficient=mesh_cross(b,c)-mesh_cross(q,d),constant_term= -mesh_cross(q,c);
    if(abs(a)<=0.0000001*abs(coefficient)) return mesh_candidate(-constant_term/coefficient,q,b,c,d,uv);
    float discriminant=mad(coefficient,coefficient,-4.0*a*constant_term);
    if(discriminant<0) return false;
    float root=sqrt(discriminant),stable= -0.5*(coefficient+(coefficient>=0?root:-root));
    if(stable==0) return mesh_candidate(-coefficient/(2.0*a),q,b,c,d,uv);
    if(mesh_candidate(stable/a,q,b,c,d,uv)) return true;
    return mesh_candidate(constant_term/stable,q,b,c,d,uv);
}
uint sample_mesh(ByteAddressBuffer paint,uint brush_base,float2 position,uint header,uint params,uint payload,uint count) {
    uint bytes;paint.GetDimensions(bytes);uint words=bytes/4u;
    if(brush_base>words) return 0u;
    words-=brush_base;
    if(payload<header||payload-header<21u||payload>words||count>words-payload) return 0u;
    uint nx=brush_word(paint,brush_base,header+4u),ny=brush_word(paint,brush_base,header+5u),patch_base=brush_word(paint,brush_base,header+6u);
    if(nx==0u||ny==0u||nx>32u||ny>32u) return 0u;
    if(patch_base<nx*ny+1u||patch_base>count) return 0u;
    float2 p=float2(mad(brush_param(paint,brush_base,params,0u),position.x,mad(brush_param(paint,brush_base,params,2u),position.y,brush_param(paint,brush_base,params,4u))),
        mad(brush_param(paint,brush_base,params,1u),position.x,mad(brush_param(paint,brush_base,params,3u),position.y,brush_param(paint,brush_base,params,5u))));
    float2 lo=float2(brush_param(paint,brush_base,params,6u),brush_param(paint,brush_base,params,7u)),hi=float2(brush_param(paint,brush_base,params,8u),brush_param(paint,brush_base,params,9u));
    if(!all(isfinite(p))||!all(isfinite(lo))||!all(isfinite(hi))||!all(isfinite(hi-lo))||any(hi<=lo)||any(p<lo)||any(p>hi)) return 0u;
    uint2 bin=min(uint2((p-lo)/(hi-lo)*float2(nx,ny)),uint2(nx-1u,ny-1u));
    uint index=bin.y*nx+bin.x,begin=brush_word(paint,brush_base,payload+index),end=brush_word(paint,brush_base,payload+index+1u);
    if(begin<nx*ny+1u||end<begin||end>patch_base) return 0u;
    uint patches=(count-patch_base)/16u;
    for(uint i=begin;i<end;++i) {
        uint patch=brush_word(paint,brush_base,payload+i);if(patch>=patches) return 0u;
        uint base=payload+patch_base+patch*16u;
        float2 a=float2(brush_param(paint,brush_base,base,0u),brush_param(paint,brush_base,base,1u)),z=float2(brush_param(paint,brush_base,base,2u),brush_param(paint,brush_base,base,3u));
        if(any(p<a)||any(p>z)||any(z<=a)) continue;
        float2 q=(p-a)/(z-a)-float2(brush_param(paint,brush_base,base,4u),brush_param(paint,brush_base,base,5u));
        float2 b=float2(brush_param(paint,brush_base,base,6u),brush_param(paint,brush_base,base,7u)),c=float2(brush_param(paint,brush_base,base,8u),brush_param(paint,brush_base,base,9u)),d=float2(brush_param(paint,brush_base,base,10u),brush_param(paint,brush_base,base,11u)),uv;
        if(!mesh_inverse(q,b,c,d,uv)) continue;
        if(brush_word(paint,brush_base,header+7u)!=0u) uv=uv*uv*(3.0-2.0*uv);
        return unorm_to_rgba8(lerp(lerp(rgba8_to_unorm(brush_word(paint,brush_base,base+12u)),rgba8_to_unorm(brush_word(paint,brush_base,base+13u)),uv.x),
            lerp(rgba8_to_unorm(brush_word(paint,brush_base,base+14u)),rgba8_to_unorm(brush_word(paint,brush_base,base+15u)),uv.x),uv.y));
    }
    return 0u;
}
#endif
