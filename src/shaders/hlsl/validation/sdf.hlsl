#include "sdf_config.hlsli"
#include "../filter/config.hlsli"
#include "../constants.hlsli"
#include "../shared/pixel.hlsli"
#include "../shared/sdf/coverage.hlsli"
#include "../scene_records.hlsli"
ConstantBuffer<FilterConfig> config:register(b0);
ByteAddressBuffer sdf_requests:register(t5);
RWByteAddressBuffer sdf_output:register(u6);
ByteAddressBuffer paint:register(t7);
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void sdf_coverage_words(uint3 gid:SV_DispatchThreadID) {
    if(gid.x>=config.pixel_count) return;
    uint address=gid.x*SDF_PROBE_REQUEST_WORDS*4u;
    uint3 request=sdf_requests.Load3(address);
    float4 affine_linear=asfloat(sdf_requests.Load4(address+SDF_PROBE_AFFINE_WORD*4u));
    float2 translation=asfloat(sdf_requests.Load2(address+SDF_PROBE_AFFINE_WORD*4u+AFFINE_TRANSLATION));
    AffineRecord inverse;
    inverse.a=affine_linear.x;inverse.b=affine_linear.y;inverse.c=affine_linear.z;inverse.d=affine_linear.w;inverse.e=translation.x;inverse.f=translation.y;
    float2 local_position=affine_record_point(inverse,asfloat(request.yz));
    float coverage=sdf_coverage_from_blob(paint,request.x,local_position.x,local_position.y,inverse);
    sdf_output.Store(gid.x*4u,coverage_to_u8(coverage));
}
