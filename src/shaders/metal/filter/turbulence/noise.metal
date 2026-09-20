int noise_wrap(int coordinate,int limit,int period) {
    if(!period) return coordinate;
    // Widen subtraction to avoid overflow for opposite signed coordinates.
    long base=long(limit)-period,relative=(long(coordinate)-base)%long(period);
    return int(base+(relative<0?relative+period:relative));
}
float noise_gradient(const device float2* gradients,uint table,uint channel,uint selector,float2 p) {
    float2 gradient=gradients[table*2056+channel*514+selector];
    return fma(gradient.x,p.x,gradient.y*p.y);
}
float perlin(const device uint* selectors,const device float2* gradients,uint table,uint channel,float2 position,bool stitch,int2 wrap,int2 extent) {
    float2 shifted=position+4096.0f;
    int2 a=int2(floor(shifted)),b=a+1;
    float2 p=shifted-float2(a);
    if(stitch) {a=int2(noise_wrap(a.x,wrap.x,extent.x),noise_wrap(a.y,wrap.y,extent.y));b=int2(noise_wrap(b.x,wrap.x,extent.x),noise_wrap(b.y,wrap.y,extent.y));}
    uint2 lower=uint2(a&255),upper=uint2(b&255);
    const device uint* table_data=selectors+table*514;
    uint left=table_data[lower.x],right=table_data[upper.x];
    float2 curve=p*p*(3.0f-2.0f*p);
    float v00=noise_gradient(gradients,table,channel,table_data[left+lower.y],p);
    float v10=noise_gradient(gradients,table,channel,table_data[right+lower.y],p-float2(1,0));
    float v01=noise_gradient(gradients,table,channel,table_data[left+upper.y],p-float2(0,1));
    float v11=noise_gradient(gradients,table,channel,table_data[right+upper.y],p-1.0f);
    float top=fma(v10-v00,curve.x,v00),bottom=fma(v11-v01,curve.x,v01);
    return fma(bottom-top,curve.y,top);
}
float stitch_frequency(float frequency,float length) {
    if(frequency<=0 || length<=0) return 0;
    float lower=floor(length*frequency)/length,upper=ceil(length*frequency)/length;
    return lower!=0 && frequency/lower<upper/frequency?lower:upper;
}
