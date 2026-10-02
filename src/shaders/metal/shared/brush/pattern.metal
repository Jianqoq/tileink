struct TextureTable { array<texture2d<float>,64> images [[id(0)]]; };
uint extend_coordinate(int value,uint size,uint mode) {
    if(size<=1) return 0;
    if(mode==1 || mode==2) {
        uint period=size*(mode==2?2:1);
        uint magnitude=value<0?0u-as_type<uint>(value):uint(value), remainder=magnitude%period;
        uint coordinate=value<0 && remainder?period-remainder:remainder;
        return coordinate<size?coordinate:period-coordinate-1;
    }
    return uint(clamp(value,0,int(size)-1));
}
float4 image_read(texture2d<float> image,uint2 p,uint page) {return image.read(p);}
float4 image_read(texture2d_array<float> image,uint2 p,uint page) {return image.read(p,page);}
float4 image_sample(texture2d<float> image,sampler sampling,float2 uv,uint page) {return image.sample(sampling,uv,level(0));}
float4 image_sample(texture2d_array<float> image,sampler sampling,float2 uv,uint page) {return image.sample(sampling,uv,page,level(0));}
template<typename Image> uint pattern_pixel(Image image,uint page,uint2 origin,uint2 size,uint extend,int2 position) {
    uint2 local(extend_coordinate(position.x,size.x,extend),extend_coordinate(position.y,size.y,extend));
    return pack_pixel(image_read(image,origin+local,page));
}
template<typename Image> uint pattern_sample(Image image,sampler sampling,float2 position,uint page,uint2 origin,uint2 size,uint opacity,uint extend,uint mode) {
    if(any(size==0)) return 0;
    uint color;
    if(mode==1 && !extend) {
        float2 uv=(float2(origin)+clamp(position,float2(0),float2(size)))/float2(image.get_width(),image.get_height());
        color=pack_pixel(image_sample(image,sampling,uv,page));
    } else if(mode==1) {
        float2 shifted=position-0.5f,base=floor(shifted),fraction=shifted-base;
        int2 p=int2(base);
        uint a=pattern_pixel(image,page,origin,size,extend,p),b=pattern_pixel(image,page,origin,size,extend,p+int2(1,0));
        uint c=pattern_pixel(image,page,origin,size,extend,p+int2(0,1)),d=pattern_pixel(image,page,origin,size,extend,p+int2(1,1));
        color=mix_pixel(mix_pixel(a,b,fraction.x),mix_pixel(c,d,fraction.x),fraction.y);
    } else color=pattern_pixel(image,page,origin,size,extend,int2(floor(position)));
    return scale_pixel(color,opacity);
}
float pattern_component(float a,float b,float offset,float2 p) {
    float by=b*p.y;
    float product=fma(a,p.x,by)+fma(b,p.y,-by);
    return product+offset;
}
uint sample_brush(Words paint,uint base,texture2d_array<float> atlas,sampler sampling,constant TextureTable& table,float2 p) {
    uint kind=paint[base],extend=paint[base+1],payload=base+paint[base+2],count=paint[base+3],params=base+9,color=paint[base+4];
    if(kind==2) return linear_gradient(paint,p,params,extend,payload,count);
    if(kind==3) return radial_gradient(paint,p,params,extend,payload,count);
    if(kind==4) return sweep_gradient(paint,p,params,extend,payload,count);
    if(kind==5) return four_corner_gradient(paint,p,params,payload);
    if(kind==6) return 0;
    if(kind!=7) return color;
    uint placement=paint[base+4];
    uint2 size(paint[base+5],paint[base+6]);
    if(any(size==0)) return 0;
    Words v=paint.offset(params);
    float2 position(pattern_component(brush_float(v,0),brush_float(v,2),brush_float(v,4),p)*float(size.x),
        pattern_component(brush_float(v,1),brush_float(v,3),brush_float(v,5),p)*float(size.y));
    if(!(placement&0x80000000u)) return pattern_sample(atlas,sampling,position,placement,uint2(paint[base+2],paint[base+3]),size,paint[base+7],extend,paint[base+8]);
    uint index=placement&0x7fffffffu;
    if(index>=64) return 0;
    return pattern_sample(table.images[index],sampling,position,0,uint2(0),size,paint[base+7],extend,paint[base+8]);
}
