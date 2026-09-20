#include "base.metal"
#include "line.metal"
#include "polygon.metal"
#include "arc.metal"
#include "effects.metal"
// Words is a bounded view of the packed scene blob, including optional records.
float sdf_blob_coverage(Words paint, uint base, float2 p, Affine inverse) {
    Words record = paint.offset(base);
    uint kind = record[0];
    float4 rect = as_type<float4>(uint4(record[1],record[2],record[3],record[4]));
    float4 radii = as_type<float4>(uint4(record[5],record[6],record[7],record[8]));
    float4 stroke = as_type<float4>(uint4(record[9],record[10],record[11],record[12]));
    float4 shadow = as_type<float4>(uint4(record[13],record[14],record[15],record[16]));
    if (kind == 1) return sdf_coverage(rect_sample(p, rect, radii), inverse);
    if (kind == 3) {
        float4 half_width = max(stroke, float4(0));
        float2 lower = min(rect.xy,rect.zw), upper = max(rect.xy,rect.zw);
        float4 corner(max(half_width.x,half_width.w), max(half_width.x,half_width.y), max(half_width.z,half_width.w), max(half_width.z,half_width.y));
        float4 outer_rect(lower.x-half_width.w,lower.y-half_width.x,upper.x+half_width.y,upper.y+half_width.z);
        float outer = sdf_coverage(rect_sample(p,outer_rect,radii+corner),inverse);
        float4 inner_rect(lower.x+half_width.w,lower.y+half_width.x,upper.x-half_width.y,upper.y-half_width.z);
        float inner = 0;
        if (inner_rect.x < inner_rect.z && inner_rect.y < inner_rect.w)
            inner = sdf_coverage(rect_sample(p,inner_rect,max(radii-corner,float4(0))),inverse);
        return clamp(outer-inner,0.0f,1.0f);
    }
    if (kind == 7) return sdf_shadow(rect_sample(p-shadow.xy,rect,radii),inverse,shadow.z,shadow.w);
    if (kind == 2) return sdf_coverage(circle_sample(p,rect.xy,rect.z),inverse);
    if (kind == 4) {
        float half_width = max(stroke.x,0.0f), radius = max(rect.z,0.0f);
        float outer = sdf_coverage(circle_sample(p,rect.xy,radius+half_width),inverse), inner = 0;
        if (radius > half_width) inner = sdf_coverage(circle_sample(p,rect.xy,radius-half_width),inverse);
        return clamp(outer-inner,0.0f,1.0f);
    }
    if (kind == 10) return sdf_shadow(circle_sample(p-shadow.xy,rect.xy,rect.z),inverse,shadow.z,shadow.w);
    if (kind == 8 || kind == 9) {
        SdfSample s = arc_sample(kind == 9 ? p-shadow.xy : p,rect.xy,rect.z,rect.w,radii.x,radii.y,radii.z);
        return kind == 9 ? sdf_shadow(s,inverse,shadow.z,shadow.w) : sdf_coverage(s,inverse);
    }
    if (kind == 5) return candlestick_coverage(p,rect.x,rect.y,rect.z,rect.w,radii.x,radii.y,radii.z,inverse);
    if (kind == 6 || kind == 11) {
        SdfSample s = line_sample(kind == 11 ? p-shadow.xy : p,rect,radii.x,radii.y);
        return kind == 11 ? sdf_shadow(s,inverse,shadow.z,shadow.w) : sdf_coverage(s,inverse);
    }
    if (kind == 12) return sdf_coverage(dash_line_sample(p,rect,radii.x,radii.y,radii.z,radii.w,stroke.x),inverse);
    if (kind == 13) return sdf_coverage(triangle_sample(p,rect.xy,rect.zw,radii.xy,radii.z),inverse);
    if (kind == 14) return sdf_coverage(checkerboard_sample(p,rect,radii.x),inverse);
    if (kind == 15 || kind == 16) {
        SdfSample s = star_sample(p,rect.xy,rect.z,rect.w,radii.x,radii.y);
        return sdf_coverage(kind == 16 ? stroke_sample(s,stroke.x) : s,inverse);
    }
    if (kind >= 17 && kind <= 19) {
        SdfSample s = callout_sample(kind == 19 ? p-shadow.xy : p,rect,radii.x,radii.y,radii.z,radii.w,stroke.x,stroke.y,stroke.w);
        if (kind == 18) s = stroke_sample(s,stroke.z);
        return kind == 19 ? sdf_shadow(s,inverse,shadow.z,shadow.w) : sdf_coverage(s,inverse);
    }
    return 0;
}
