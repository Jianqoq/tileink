// Particle and glyph allocation use the same ordered, wrapping-u32 scan.
groupshared uint2 scratch[COARSE_WORKGROUP_SIZE];
groupshared uint2 block_total;

uint2 exclusive_prefix(uint2 value, uint lane) {
    scratch[lane] = value;
    GroupMemoryBarrierWithGroupSync();
    for (uint step = 1u; step < COARSE_WORKGROUP_SIZE; step *= 2u) {
        uint index = (lane + 1u) * step * 2u - 1u;
        if (index < COARSE_WORKGROUP_SIZE) scratch[index] += scratch[index - step];
        GroupMemoryBarrierWithGroupSync();
    }
    if (lane == 0u) {
        block_total = scratch[COARSE_WORKGROUP_SIZE - 1u];
        scratch[COARSE_WORKGROUP_SIZE - 1u] = uint2(0u, 0u);
    }
    GroupMemoryBarrierWithGroupSync();
    for (uint step = COARSE_WORKGROUP_SIZE / 2u; step > 0u; step /= 2u) {
        uint index = (lane + 1u) * step * 2u - 1u;
        if (index < COARSE_WORKGROUP_SIZE) {
            uint left = index - step;
            uint2 previous = scratch[left];
            scratch[left] = scratch[index];
            scratch[index] += previous;
        }
        GroupMemoryBarrierWithGroupSync();
    }
    uint2 result = scratch[lane];
    GroupMemoryBarrierWithGroupSync();
    return result;
}

