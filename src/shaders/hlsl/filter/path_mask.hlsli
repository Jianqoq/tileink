#ifndef TILEINK_PATH_MASK_HLSLI
#define TILEINK_PATH_MASK_HLSLI
#include "../constants.hlsli"
// Root fix: compare exact signed products, avoiding float conversion and division.
// Two 32-bit words preserve every product bit without requiring shader int64 support.
uint2 path_mask_multiply(uint a,uint b) {
    uint a0=a&65535u,a1=a>>16u,b0=b&65535u,b1=b>>16u;
    uint t=a0*b0;
    uint low=t&65535u,carry=t>>16u;
    t=a1*b0+carry;
    uint middle=t&65535u,high=t>>16u;
    t=a0*b1+middle;
    return uint2((t<<16u)+low,a1*b1+high+(t>>16u));
}
struct PathMaskProduct {uint2 magnitude;bool negative;};
PathMaskProduct path_mask_product(int a,int b,int c,int d) {
    bool ab_negative=a<b,cd_negative=c<d;
    uint ab=ab_negative ? asuint(b)-asuint(a) : asuint(a)-asuint(b);
    uint cd=cd_negative ? asuint(d)-asuint(c) : asuint(c)-asuint(d);
    PathMaskProduct result;
    result.magnitude=path_mask_multiply(ab,cd);
    result.negative=(ab_negative!=cd_negative) && ab!=0u && cd!=0u;
    return result;
}
int path_mask_orientation(int2 start,int2 end,int2 position) {
    PathMaskProduct a=path_mask_product(start.x,position.x,end.y,position.y);
    PathMaskProduct b=path_mask_product(end.x,position.x,start.y,position.y);
    if(a.negative!=b.negative) return a.negative ? -1 : 1;
    int order=0;
    if(a.magnitude.y!=b.magnitude.y) order=a.magnitude.y>b.magnitude.y ? 1 : -1;
    else if(a.magnitude.x!=b.magnitude.x) order=a.magnitude.x>b.magnitude.x ? 1 : -1;
    return a.negative ? -order : order;
}
#endif
