// Brush records use bounded relative word offsets; empty payloads are transparent.
float brush_float(Words words,uint index) {return as_type<float>(words[index]);}
float gradient_extend(float t,uint mode) {
    if(mode==1) return fma(-floor(t),1.0f,t);
    if(mode==2) {float value=fma(-floor(t/2.0f),2.0f,t);return value<=1?value:2.0f-value;}
    return clamp(t,0.0f,1.0f);
}
uint gradient_ramp(Words paint,uint payload,uint count,float t,uint extend) {
    if(!count) return 0;
    uint last=count-1;
    float position=gradient_extend(t,extend)*float(last);
    uint a=uint(floor(position)),b=min(a+1,last);
    float fraction=position-float(a);
    uint left=paint[payload+a],right=paint[payload+b];
    return fraction<=0.00000011920929f || a==b ? left : mix_pixel(left,right,fraction);
}
uint linear_gradient(Words paint,float2 p,uint base,uint extend,uint payload,uint count) {
    Words v=paint.offset(base);
    float tx=fma(brush_float(v,4),p.x,fma(brush_float(v,6),p.y,brush_float(v,8)));
    float ty=fma(brush_float(v,5),p.x,fma(brush_float(v,7),p.y,brush_float(v,9)));
    float2 start(brush_float(v,0),brush_float(v,1)),end(brush_float(v,2),brush_float(v,3)),delta=end-start;
    float denominator=delta.x*delta.x+delta.y*delta.y,t=0;
    if(denominator>0.00000011920929f) t=fma(tx-start.x,delta.x,(ty-start.y)*delta.y)/denominator;
    return gradient_ramp(paint,payload,count,t,extend);
}
uint radial_gradient(Words paint,float2 p,uint base,uint extend,uint payload,uint count) {
    Words v=paint.offset(base);
    float tx=fma(brush_float(v,6),p.x,fma(brush_float(v,8),p.y,brush_float(v,10)));
    float ty=fma(brush_float(v,7),p.x,fma(brush_float(v,9),p.y,brush_float(v,11)));
    float2 start(brush_float(v,0),brush_float(v,1)),end(brush_float(v,2),brush_float(v,3));
    float r0=brush_float(v,4),r1=brush_float(v,5),dr=r1-r0;
    float2 q=float2(tx,ty)-start,dc=end-start;
    float a=fma(dc.x,dc.x,fma(dc.y,dc.y,-dr*dr));
    float b=-2.0f*fma(q.x,dc.x,fma(q.y,dc.y,r0*dr));
    float c=fma(q.x,q.x,fma(q.y,q.y,-r0*r0));
    float t=0;bool valid=false;
    if(abs(a)<=0.000001f) {
        if(abs(b)>0.000001f) {float candidate=-c/b;if(fma(candidate,dr,r0)>=0){valid=true;t=candidate;}}
    } else {
        float discriminant=fma(b,b,-4.0f*a*c);
        if(discriminant>=0) {
            float root=sqrt(discriminant),t0=(-b-root)/(2.0f*a),t1=(-b+root)/(2.0f*a);
            bool valid0=fma(t0,dr,r0)>=0,valid1=fma(t1,dr,r0)>=0;
            if(valid0){valid=true;t=valid1?max(t0,t1):t0;}else if(valid1){valid=true;t=t1;}
        }
    }
    return valid?gradient_ramp(paint,payload,count,t,extend):0;
}
uint sweep_gradient(Words paint,float2 p,uint base,uint extend,uint payload,uint count) {
    Words v=paint.offset(base);
    float2 center(brush_float(v,0),brush_float(v,1));
    float start=brush_float(v,2),end=brush_float(v,3),span=end-start,t=0;
    if(abs(span)>0.00000011920929f) {
        float angle=all(p==center)?0.0f:atan2(p.y-center.y,p.x-center.x);
        if(span>0){while(angle<start) angle+=6.2831855f;}else{while(angle>start) angle-=6.2831855f;}
        t=(angle-start)/span;
    }
    return gradient_ramp(paint,payload,count,t,extend);
}
uint four_corner_gradient(Words paint,float2 p,uint base,uint payload) {
    Words v=paint.offset(base);
    float2 a(brush_float(v,0),brush_float(v,1)),b(brush_float(v,2),brush_float(v,3)),size=b-a;
    float u=abs(size.x)>0.00000011920929f?clamp((p.x-a.x)/size.x,0.0f,1.0f):0;
    float vcoord=abs(size.y)>0.00000011920929f?clamp((p.y-a.y)/size.y,0.0f,1.0f):0;
    return mix_pixel(mix_pixel(paint[payload],paint[payload+1],u),mix_pixel(paint[payload+3],paint[payload+2],u),vcoord);
}
