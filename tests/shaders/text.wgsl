@group(0) @binding(9) var<storage,read> text_requests: array<u32>;
@group(0) @binding(10) var<storage,read_write> text_output: array<u32>;
struct TextRequestConfig { count:u32, pad0:u32, pad1:u32, pad2:u32 };
@group(0) @binding(11) var<uniform> text_config: TextRequestConfig;
@compute @workgroup_size(FINE_WORKGROUP_SIZE,1,1)
fn text_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if (id.x >= text_config.count) { return; }
    let p=id.x*4u;
    let dst=text_requests[p]; let src=text_requests[p+1u];
    let mask=text_requests[p+2u]; let clip=text_requests[p+3u];
    let coverage=combine_alpha(mask & 255u,clip);
    let base=id.x*5u;
    text_output[base]=src_over_subpixel_mask_u8(dst,src,mask,clip);
    text_output[base+1u]=src_over_mask_linear_u8(dst,src,coverage);
    text_output[base+2u]=src_over_mask_linear_auto_u8(dst,src,coverage);
    text_output[base+3u]=src_over_subpixel_mask_linear_u8(dst,src,mask,clip);
    text_output[base+4u]=src_over_subpixel_mask_linear_auto_u8(dst,src,mask,clip);
}
