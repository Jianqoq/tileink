#ifndef TILEINK_HLSL_COARSE_PREFIX_SCAN_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_PREFIX_SCAN_HLSLI_INCLUDED

#include "../constants.hlsli"
// Particle and glyph allocation use the same ordered, wrapping-u32 scan.
// Workgroup scratch is private to this helper; both prefix and total are explicit outputs.
groupshared uint2 coarse_prefix_scratch[COARSE_WORKGROUP_SIZE];
groupshared uint2 coarse_prefix_total;

uint2 exclusive_prefix(uint2 value, uint lane, out uint2 total) {
    coarse_prefix_scratch[lane] = value;
    GroupMemoryBarrierWithGroupSync();
    for (uint upsweep_step = 1u; upsweep_step < COARSE_WORKGROUP_SIZE; upsweep_step *= 2u) {
        uint index = (lane + 1u) * upsweep_step * 2u - 1u;
        if (index < COARSE_WORKGROUP_SIZE) coarse_prefix_scratch[index] += coarse_prefix_scratch[index - upsweep_step];
        GroupMemoryBarrierWithGroupSync();
    }
    if (lane == 0u) {
        coarse_prefix_total = coarse_prefix_scratch[COARSE_WORKGROUP_SIZE - 1u];
        coarse_prefix_scratch[COARSE_WORKGROUP_SIZE - 1u] = uint2(0u, 0u);
    }
    GroupMemoryBarrierWithGroupSync();
    for (uint downsweep_step = COARSE_WORKGROUP_SIZE / 2u; downsweep_step > 0u; downsweep_step /= 2u) {
        uint index = (lane + 1u) * downsweep_step * 2u - 1u;
        if (index < COARSE_WORKGROUP_SIZE) {
            uint left = index - downsweep_step;
            uint2 previous = coarse_prefix_scratch[left];
            coarse_prefix_scratch[left] = coarse_prefix_scratch[index];
            coarse_prefix_scratch[index] += previous;
        }
        GroupMemoryBarrierWithGroupSync();
    }
    uint2 result = coarse_prefix_scratch[lane];
    total = coarse_prefix_total;
    GroupMemoryBarrierWithGroupSync();
    return result;
}

#endif // TILEINK_HLSL_COARSE_PREFIX_SCAN_HLSLI_INCLUDED
