float compose_source(uint mode,float alpha) {
    if(mode==0 || mode==2 || mode==6 || mode==8) return 0;
    if(mode==4 || mode==7 || mode==10 || mode==11) return 1.0f-alpha;
    return mode==5 || mode==9?alpha:1.0f;
}
float compose_destination(uint mode,float alpha) {
    if(mode==0 || mode==1 || mode==5 || mode==7) return 0;
    if(mode==2 || mode==4 || mode==12 || mode==13) return 1;
    return mode==6 || mode==10?alpha:1.0f-alpha;
}
float premul_dodge(float s,float d,float sa,float da) {
    float result=s*(1.0f-da);
    if(d>0) result=s>=sa?s+d*(1.0f-sa):sa*min(da,(d*sa)/(sa-s))+s*(1.0f-da)+d*(1.0f-sa);
    return result;
}
float premul_burn(float s,float d,float sa,float da) {
    float result=d+s*(1.0f-da);
    if(d<da) result=s<=0?d*(1.0f-sa):sa*(da-min(da,((da-d)*sa)/s))+s*(1.0f-da)+d*(1.0f-sa);
    return result;
}
uint blend_pixel(uint destination,uint source,uint mode) {
    uint mixing=mode&255,compose=(mode>>8)&255;
    float4 s=unpack_pixel(source),d=unpack_pixel(destination),result=s+d*(1.0f-s.a);
    if(!mixing) {
        if(compose==2) result=d;
        else if(compose==0) result=float4(0);
        else if(compose==1) result=s;
        else if(compose!=3) {
            result=s*compose_source(compose,d.a)+d*compose_destination(compose,s.a);
            if(compose==13) result=min(result,1.0f);
        }
    } else if(compose==3 && mixing==6) {
        result.rgb=float3(premul_dodge(s.r,d.r,s.a,d.a),premul_dodge(s.g,d.g,s.a,d.a),premul_dodge(s.b,d.b,s.a,d.a));
    } else if(compose==3 && mixing==7) {
        result.rgb=float3(premul_burn(s.r,d.r,s.a,d.a),premul_burn(s.g,d.g,s.a,d.a),premul_burn(s.b,d.b,s.a,d.a));
    } else {
        float sa=clamp(s.a,0.0f,1.0f),da=clamp(d.a,0.0f,1.0f);
        float3 straight_s=sa>0?s.rgb/sa:float3(0),straight_d=da>0?d.rgb/da:float3(0);
        float3 mixed=mix_color(straight_d,straight_s,mixing);
        float3 effective=sa*fma(da,mixed,(1.0f-da)*straight_s);
        float sf=compose_source(compose,da),df=compose_destination(compose,sa);
        result.rgb=effective*sf+d.rgb*df;result.a=sa*sf+da*df;
    }
    return pack_pixel(result);
}
