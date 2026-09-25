struct Group {uint kind;float4 parent;uint clip,alpha,payload;};
struct PixelStack {uint mask,clip_depth,group_depth;uint clip[4];Group groups[2];};
void push_clip(FineInput input,uint tile,uint lane,thread PixelStack& stack) {
    uint depth=stack.clip_depth;
    if(depth<4) stack.clip[depth]=stack.mask;
    else {
        uint spill=depth-4;
        if(spill>=input.config->clip_spill_depth) return;
        input.spills[(tile*input.config->clip_spill_depth+spill)*256+lane]=stack.mask;
    }
    ++stack.clip_depth;
}
void pop_clip(FineInput input,uint tile,uint lane,thread PixelStack& stack) {
    if(!stack.clip_depth){stack.mask=255;return;}
    uint depth=--stack.clip_depth;
    if(depth<4) stack.mask=stack.clip[depth];
    else if(depth-4<input.config->clip_spill_depth) stack.mask=input.spills[(tile*input.config->clip_spill_depth+depth-4)*256+lane];
}
uint group_offset(FineInput input,uint tile,uint lane,uint depth) {
    return input.config->group_spill_base+((tile*input.config->group_spill_depth+depth)*256+lane)*5;
}
bool push_group(FineInput input,uint tile,uint lane,thread PixelStack& stack,Group group) {
    uint depth=stack.group_depth;
    if(depth<2) stack.groups[depth]=group;
    else {
        uint spill=depth-2;
        if(spill>=input.config->group_spill_depth) return false;
        device uint* p=input.spills+group_offset(input,tile,lane,spill);
        p[0]=group.kind;p[1]=pack_pixel(group.parent);p[2]=group.clip;p[3]=group.alpha;p[4]=group.payload;
    }
    ++stack.group_depth;
    return true;
}
bool pop_group(FineInput input,uint tile,uint lane,thread PixelStack& stack,thread Group& group) {
    if(!stack.group_depth) return false;
    uint depth=--stack.group_depth;
    if(depth<2) group=stack.groups[depth];
    else {
        uint spill=depth-2;
        if(spill>=input.config->group_spill_depth) return false;
        const device uint* p=input.spills+group_offset(input,tile,lane,spill);
        group=Group{p[0],unpack_pixel(p[1]),p[2],p[3],p[4]};
    }
    return true;
}
