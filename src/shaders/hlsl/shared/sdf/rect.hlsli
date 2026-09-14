#ifndef TILEINK_HLSL_SDF_RECT_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_RECT_HLSLI_INCLUDED
float rect_sdf_distance(float2 position,float4 bounds,float4 radii) {
    float2 lower=min(bounds.xy,bounds.zw),upper=max(bounds.xy,bounds.zw);
    float2 center=(lower+upper)*0.5,half_size=(upper-lower)*0.5;
    float2 relative_position=position-center;
    float radius=relative_position.x>=0.0 ? (relative_position.y<=0.0 ? radii.y : radii.w) : (relative_position.y>0.0 ? radii.z : radii.x);
    radius=max(min(min(radius,half_size.x),half_size.y),0.0);
    float2 q=abs(relative_position)-half_size+radius;
    return min(max(q.x,q.y),0.0)+length(max(q,0.0))-radius;
}
float sdf_coverage_from_dist(float distance) { return clamp(0.5-distance,0.0,1.0); }
#endif
