#include "constants.hlsli"
#include "shared/range_scatter_constants.hlsli"

// Shared packed range-upload layout, with explicit native binding slots:
// Source records use t1 / binding 1; destination words use u0 / binding 0.
ByteAddressBuffer source : register(t1);
RWByteAddressBuffer destination : register(u0);

[numthreads(RANGE_SCATTER_WORKGROUP_SIZE, 1, 1)]
void range_scatter(uint3 group : SV_GroupID, uint3 local : SV_GroupThreadID) {
    uint descriptor = RANGE_SCATTER_HEADER_WORDS + group.x * RANGE_SCATTER_DESCRIPTOR_WORDS;
    uint payload = source.Load(0);
    uint dst = source.Load(descriptor * 4u);
    uint src = source.Load((descriptor + 1u) * 4u);
    uint len = source.Load((descriptor + 2u) * 4u);
    for (uint offset = local.x; offset < len; offset += RANGE_SCATTER_WORKGROUP_SIZE) {
        destination.Store((dst + offset) * 4u, source.Load((payload + src + offset) * 4u));
    }
}
