// Signed area accumulation preserves the reference's separate base/running/partial
// sums and explicit endpoint/intersection rounding at half-alpha boundaries.
float4 segment_row(float4 p, float edge, uint y) {
    float dx=p.z-p.x,dy=p.w-p.y,row=float(y),local=p.y-row;
    float y0=clamp(local,0.0f,1.0f),y1=clamp(p.w-row,0.0f,1.0f),delta=y0-y1;
    float row_edge=(dx<0?-1.0f:1.0f)*clamp(row-edge+1.0f,0.0f,1.0f);
    if(delta==0) return float4(row_edge,delta,0,0);
    bool near=abs(local-0.5f)<=abs(p.w-row-0.5f);
    float anchor_x=near?p.x:p.z,anchor_y=near?local:p.w-row,slope=dx/dy;
    float a=fma(y0-anchor_y,slope,anchor_x),b=fma(y1-anchor_y,slope,anchor_x);
    return float4(row_edge,delta,min(a,b),max(a,b));
}
float segment_area(float a,float b,uint x) {
    float lower=a-float(x),upper=b-float(x),width=upper-lower;
    if(width==0) return clamp(1.0f-lower,0.0f,1.0f);
    if(lower>=0 && upper<=1) return fma(-0.5f,lower+upper,1.0f);
    float left=clamp(lower,0.0f,1.0f),right=clamp(upper,0.0f,1.0f),full=clamp(-lower,0.0f,width);
    return fma(right-left,fma(-0.5f,left+right,1.0f),full)/width;
}
uint fill_alpha(const device float* segments,int backdrop,uint rule,uint start,uint end,uint2 xy) {
    float base=float(backdrop),running=0,partial=0;
    for(uint i=start;i<end;++i) {
        const device float* s=segments+i*5;
        float4 p=segment_row(float4(s[0],s[1],s[2],s[3]),s[4],xy.y);
        base+=p.x;
        if(p.y!=0) {
            int full_start=clamp(int(ceil(p.w)),0,16);
            if(full_start<16 && int(xy.x)>=full_start) running+=p.y;
            int first=clamp(int(floor(p.z)),0,16),last=clamp(int(ceil(p.w)),0,16);
            if(int(xy.x)>=first && int(xy.x)<last) partial+=segment_area(p.z,p.w,xy.x)*p.y;
        }
    }
    float value=base+running+partial;
    return coverage_u8(rule==1?abs(value-2.0f*rint(0.5f*value)):min(abs(value),1.0f));
}
