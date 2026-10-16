#include <metal_stdlib>
using namespace metal;
#include "region.metal"
// Exact signed determinant using Metal's 64-bit integer arithmetic. Coordinates
// may span all of i32, so differences are widened before multiplication and
// magnitudes are unsigned (the product can exceed signed 64-bit range).
struct SignedProduct {ulong magnitude;bool negative;};
SignedProduct difference_product(int a,int b,int c,int d) {
    long x=long(a)-long(b),y=long(c)-long(d);
    return {ulong(x<0?-x:x)*ulong(y<0?-y:y),(x<0)!=(y<0) && x!=0 && y!=0};
}
int orientation(int2 a,int2 b,int2 p) {
    SignedProduct x=difference_product(a.x,p.x,b.y,p.y),y=difference_product(b.x,p.x,a.y,p.y);
    if(x.negative!=y.negative) return x.negative?-1:1;
    int order=x.magnitude>y.magnitude?1:(x.magnitude<y.magnitude?-1:0);
    return x.negative?-order:order;
}
kernel void filter_path_mask_region(constant FilterConfig& config [[buffer(0)]],texture2d<float,access::write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],const device uint* starts [[buffer(9)]],const device uint* ends [[buffer(10)]],
    const device int* ax [[buffer(11)]],const device int* ay [[buffer(12)]],const device int* bx [[buffer(13)]],const device int* by [[buffer(14)]],
    uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    int2 position=int2(xy*256+128);int winding=0;
    for(uint i=starts[config.table_index];i<ends[config.table_index];++i) {
        int2 a(ax[i],ay[i]),b(bx[i],by[i]);
        int delta=a.y<=position.y && b.y>position.y?1:(b.y<=position.y && a.y>position.y?-1:0);
        if(delta && orientation(a,b,position)==delta) winding+=delta;
    }
    target.write(float4(winding!=0?1.0f:0.0f),xy);
}
