#include "../constants.hlsli"
#include "../shared/blend.hlsli"
struct MathConfig { uint count; uint pad0; uint pad1; uint pad2; };
ConstantBuffer<MathConfig> config : register(b0);
ByteAddressBuffer source : register(t1);
RWByteAddressBuffer destination : register(u2);
[numthreads(FINE_WORKGROUP_SIZE,1,1)]
void blend_math_words(uint3 id : SV_DispatchThreadID) {
    if (id.x>=config.count) return;
    uint3 record=source.Load3(id.x*16u);
    destination.Store(id.x*4u,blend_premul_u8(record.y,record.x,record.z));
}
