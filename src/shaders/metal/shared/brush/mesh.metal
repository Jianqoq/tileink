// Bilinear quad inversion uses a stable quadratic solve, not an iteration count
// that could leave holes in skewed patches. Patch coordinates are normalized
// independently in x/y during compilation to avoid aspect-ratio conditioning.
float mesh_cross(float2 a,float2 b) {return fma(a.x,b.y,-a.y*b.x);}
bool mesh_candidate(float u,float2 q,float2 b,float2 c,float2 d,thread float2& uv) {
    float2 axis=c+d*u,rest=q-b*u;
    float v=abs(axis.x)>abs(axis.y)?rest.x/axis.x:rest.y/axis.y;
    if(!isfinite(u)||!isfinite(v)) return false;
    float2 du=b+d*v;
    float determinant=abs(mesh_cross(du,axis));
    if(!isfinite(determinant)||determinant<=0) return false;
    // Convert normalized-coordinate rounding into parameter error through the
    // inverse Jacobian; a fixed UV epsilon creates holes in thin valid quads.
    float2 tolerance=max(float2(0.00002),float2(abs(axis.x)+abs(axis.y),abs(du.x)+abs(du.y))*(0.0000009536743164/determinant));
    if(any(float2(u,v)< -tolerance)||any(float2(u,v)>1.0+tolerance)) return false;
    uv=clamp(float2(u,v),0.0f,1.0f);return true;
}
bool mesh_inverse(float2 q,float2 b,float2 c,float2 d,thread float2& uv) {
    float a=mesh_cross(b,d),coefficient=mesh_cross(b,c)-mesh_cross(q,d),constant_term= -mesh_cross(q,c);
    if(abs(a)<=0.0000001f*abs(coefficient)) return mesh_candidate(-constant_term/coefficient,q,b,c,d,uv);
    float discriminant=fma(coefficient,coefficient,-4.0f*a*constant_term);
    if(discriminant<0) return false;
    float root=sqrt(discriminant),stable= -0.5f*(coefficient+(coefficient>=0?root:-root));
    if(stable==0) return mesh_candidate(-coefficient/(2.0f*a),q,b,c,d,uv);
    if(mesh_candidate(stable/a,q,b,c,d,uv)) return true;
    return mesh_candidate(constant_term/stable,q,b,c,d,uv);
}
uint mesh_gradient(Words paint,float2 position,uint header,uint params,uint payload,uint count) {
    if(payload<header||payload-header<21||payload>paint.length||count>paint.length-payload) return 0;
    uint nx=paint[header+4],ny=paint[header+5],patch_base=paint[header+6];
    if(!nx||!ny||nx>32||ny>32||patch_base<nx*ny+1||patch_base>count) return 0;
    Words v=paint.offset(params);
    float2 p(fma(brush_float(v,0),position.x,fma(brush_float(v,2),position.y,brush_float(v,4))),
        fma(brush_float(v,1),position.x,fma(brush_float(v,3),position.y,brush_float(v,5))));
    float2 lo(brush_float(v,6),brush_float(v,7)),hi(brush_float(v,8),brush_float(v,9));
    if(!all(isfinite(p))||!all(isfinite(lo))||!all(isfinite(hi))||!all(isfinite(hi-lo))||any(hi<=lo)||any(p<lo)||any(p>hi)) return 0;
    uint2 bin=min(uint2((p-lo)/(hi-lo)*float2(nx,ny)),uint2(nx-1,ny-1));
    uint index=bin.y*nx+bin.x,begin=paint[payload+index],end=paint[payload+index+1];
    if(begin<nx*ny+1||end<begin||end>patch_base) return 0;
    uint patches=(count-patch_base)/16;
    for(uint i=begin;i<end;++i) {
        uint patch=paint[payload+i];if(patch>=patches) return 0;
        uint base=payload+patch_base+patch*16;
        Words w=paint.offset(base);
        float2 a(brush_float(w,0),brush_float(w,1)),z(brush_float(w,2),brush_float(w,3));
        if(any(p<a)||any(p>z)||any(z<=a)) continue;
        float2 q=(p-a)/(z-a)-float2(brush_float(w,4),brush_float(w,5));
        float2 b(brush_float(w,6),brush_float(w,7)),c(brush_float(w,8),brush_float(w,9)),d(brush_float(w,10),brush_float(w,11)),uv;
        if(!mesh_inverse(q,b,c,d,uv)) continue;
        if(paint[header+7]) uv=uv*uv*(3.0f-2.0f*uv);
        return pack_pixel(mix(mix(unpack_pixel(w[12]),unpack_pixel(w[13]),uv.x),
            mix(unpack_pixel(w[14]),unpack_pixel(w[15]),uv.x),uv.y));
    }
    return 0;
}
