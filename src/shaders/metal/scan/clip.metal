float intersection_x(float4 points,float y) {return fma(points.z-points.x,(y-points.y)/(points.w-points.y),points.x);}
float intersection_y(float4 points,float x) {return fma(points.w-points.y,(x-points.x)/(points.z-points.x),points.y);}
float clipped_y(float4 points,float x,float top,float bottom) {
    float y=intersection_y(points,x);
    if(y<=top+1.0e-6f) return abs(intersection_x(points,top)-x)>0.001f?top:top+0.001f;
    return clamp(y,top+0.001f,bottom);
}
float snap_endpoint(float value) {
    value=clamp(value,0.0f,16.0f);
    if(value<=1.0e-6f) return 0;
    return 16.0f-value<=1.0e-6f?16.0f:value;
}
void emit_segment(device float* segments,uint destination,Traversal scan,uint index,float z,int2 tile) {
    float2 minimum=float2(tile)*16.0f,maximum=minimum+16.0f;
    float4 points=scan.points;
    if(index>0) {
        float previous=floor(scan.a*(float(index)-1.0f)+scan.b);
        if(z==previous) points.xy=float2(clamp(intersection_x(scan.points,minimum.y),minimum.x+0.001f,maximum.x),minimum.y);
        else {float x=scan.direction>0?minimum.x:maximum.x;points.xy=float2(x,clipped_y(scan.points,x,minimum.y,maximum.y));}
    }
    if(index+1<scan.count) {
        float next=floor(scan.a*(float(index)+1.0f)+scan.b);
        if(z==next) points.zw=float2(clamp(intersection_x(scan.points,maximum.y),minimum.x+0.001f,maximum.x),maximum.y);
        else {float x=scan.direction>0?maximum.x:minimum.x;points.zw=float2(x,clipped_y(scan.points,x,minimum.y,maximum.y));}
    }
    float4 local=float4(snap_endpoint(points.x-minimum.x),snap_endpoint(points.y-minimum.y),snap_endpoint(points.z-minimum.x),snap_endpoint(points.w-minimum.y));
    float edge=1.0e9f;
    if(local.x==0) {
        if(local.z==0) {
            local.x=1.0e-6f;
            if(local.y==0) {local.z=1.0e-6f;local.w=16.0f;}
            else {local.z=2.0e-6f;local.w=local.y;}
        } else if(local.y==0) local.x=1.0e-6f;
        else edge=local.y;
    } else if(local.z==0) {
        if(local.w==0) local.z=1.0e-6f;else edge=local.w;
    }
    if(floor(local.x)==local.x && local.x!=0) local.x-=1.0e-6f;
    if(floor(local.z)==local.z && local.z!=0) local.z-=1.0e-6f;
    if(!scan.down) local=local.zwxy;
    uint offset=destination*5;
    segments[offset]=local.x;segments[offset+1]=local.y;segments[offset+2]=local.z;segments[offset+3]=local.w;segments[offset+4]=edge;
}
