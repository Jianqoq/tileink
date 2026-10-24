struct Affine { float4 linear; float2 translation; };
float2 affine_point(Affine a, float2 p) {
    return float2(fma(a.linear.x, p.x, fma(a.linear.z, p.y, a.translation.x)),
        fma(a.linear.y, p.x, fma(a.linear.w, p.y, a.translation.y)));
}
