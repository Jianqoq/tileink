// Exact signed-product comparison preserves fixed-point crossings without int64.
fn path_mask_multiply(a:u32,b:u32)->vec2<u32> {
    let a0=a&65535u;let a1=a>>16u;let b0=b&65535u;let b1=b>>16u;
    var t=a0*b0;
    let low=t&65535u;let carry=t>>16u;
    t=a1*b0+carry;
    let middle=t&65535u;let high=t>>16u;
    t=a0*b1+middle;
    return vec2<u32>((t<<16u)+low,a1*b1+high+(t>>16u));
}
struct PathMaskProduct {magnitude:vec2<u32>,negative:bool,}
fn path_mask_product(a:i32,b:i32,c:i32,d:i32)->PathMaskProduct {
    let ab_negative=a<b;let cd_negative=c<d;
    let ab=select(bitcast<u32>(a)-bitcast<u32>(b),bitcast<u32>(b)-bitcast<u32>(a),ab_negative);
    let cd=select(bitcast<u32>(c)-bitcast<u32>(d),bitcast<u32>(d)-bitcast<u32>(c),cd_negative);
    return PathMaskProduct(path_mask_multiply(ab,cd),(ab_negative!=cd_negative)&&ab!=0u&&cd!=0u);
}
fn path_mask_orientation(start:vec2<i32>,end:vec2<i32>,position:vec2<i32>)->i32 {
    let a=path_mask_product(start.x,position.x,end.y,position.y);
    let b=path_mask_product(end.x,position.x,start.y,position.y);
    if(a.negative!=b.negative) {return select(1i,-1i,a.negative);}
    var order=0i;
    if(a.magnitude.y!=b.magnitude.y) {order=select(-1i,1i,a.magnitude.y>b.magnitude.y);}
    else if(a.magnitude.x!=b.magnitude.x) {order=select(-1i,1i,a.magnitude.x>b.magnitude.x);}
    return select(order,-order,a.negative);
}
