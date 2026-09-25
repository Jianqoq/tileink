float glass_distance(constant FilterConfig& c,float2 p) {
    float2 relative=p-(float2(c.rect_x0,c.rect_y0)+float2(c.rect_x1,c.rect_y1))*0.5f;
    float2 half_size=max((float2(c.rect_x1,c.rect_y1)-float2(c.rect_x0,c.rect_y0))*0.5f,0.0f);
    float radius=relative.x>=0?(relative.y<=0?c.radius_top_right:c.radius_bottom_right):(relative.y>0?c.radius_bottom_left:c.radius_top_left);
    radius=max(min(min(radius,half_size.x),half_size.y),0.0f);
    float2 edge=abs(relative)-half_size;
    // Keep square and rounded corners as separate distance evaluations. The
    // normal takes finite differences; merging these expressions permits
    // radius cancellation before the neighboring distances have rounded.
    if(radius<=0) {
        float2 outside=max(edge,0.0f);
        return sqrt(outside.x*outside.x+outside.y*outside.y)+min(max(edge.x,edge.y),0.0f);
    }
    float2 q=edge+radius,outside=max(q,0.0f);
    return min(max(q.x,q.y),0.0f)+sqrt(outside.x*outside.x+outside.y*outside.y)-radius;
}
float2 glass_normal(constant FilterConfig& c,float2 p) {
    float2 n(glass_distance(c,p+float2(1,0))-glass_distance(c,p-float2(1,0)),
        glass_distance(c,p+float2(0,1))-glass_distance(c,p-float2(0,1)));
    float length=sqrt(n.x*n.x+n.y*n.y);return length>0.000001f?n/length:float2(0,-1);
}
float glass_edge(float distance,float thickness,float factor) {
    thickness=max(thickness,0.000001f);factor=max(factor,1.0f);
    if(distance>=thickness || factor==1) return 0;
    float ratio=clamp(1.0f-distance/thickness,0.0f,1.0f),incident=ratio*ratio,transmitted=incident/factor;
    float ct=sqrt(max(fma(-transmitted,transmitted,1.0f),0.0f));
    if(incident==1) return factor*ct;
    float ci=sqrt(max(fma(-incident,incident,1.0f),0.0f));
    return max(fma(incident,ct,-(ci*transmitted))/fma(ci,ct,incident*transmitted),0.0f);
}
float glass_highlight(float distance,float range,float hardness) {
    float base=1.0f+distance/1500.0f*pow(500.0f/max(range,0.000001f),2.0f)+hardness;
    return pow(clamp(base,0.0f,1.0f),5.0f);
}
float glass_glare(constant FilterConfig& c,float2 normal) {
    float angle=0;
    if(sqrt(normal.x*normal.x+normal.y*normal.y)>=0.00000001f) {angle=atan2(normal.y,normal.x);if(angle<0) angle+=2.0f*3.1415927f;}
    angle=(angle-3.1415927f*0.25f+c.liquid_glare_angle)*2.0f;
    float side=1.2f;
    if((angle>3.1415927f*1.5f && angle<3.1415927f*3.5f) || angle<-3.1415927f*0.5f) side*=c.liquid_glare_opposite_factor;
    return clamp(pow((0.5f+sin(angle)*0.5f)*side*c.liquid_glare_factor,0.1f+c.liquid_glare_convergence*2.0f),0.0f,1.0f);
}
