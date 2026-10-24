#ifndef TILEINK_METAL_COARSE_OCCLUSION_INCLUDED
#define TILEINK_METAL_COARSE_OCCLUSION_INCLUDED

#include "../shared/tile_kinds.metal"

// Later opaque colors can hide work; a lone background cannot.
constant uint CHUNK_CLASS_OCCLUDER = 8u;

// Same full-coverage and stream-ownership contract as HLSL occlusion.hlsli.
// Wrappers forbid trimming: partial clips apply coverage to each draw, while
// opacity/blend groups must retain the parent pixel saved by their begin tag.
uint occluded_particle_prefix(device uint* work, constant CoarseConfig& c,
    uint tile, uint end, uint wrappers) {
    uint start = work[tile * 6 + 1];
    if (wrappers || end <= start || end - start <= 2
        || end > c.ptcl_capacity || end > work[tile * 6 + 2]) return 0u;
    const device uint* particles = work + c.tile_count * 6;
    if (particles[(end - 1) * 6] != 0) return 0u;
    uint depth = 0;
    for (uint cursor = end - 1; cursor > start;) {
        const device uint* p = particles + --cursor * 6;
        uint tag = p[0];
        if (tag == 4 || tag == 6 || tag == 8) ++depth;
        else if (tag == 3 || tag == 12 || tag == 5 || tag == 7) {
            if (!depth) return 0u;
            --depth;
        } else if (tag == 2 && !depth && (p[5] >> 24) == 255) {
            return cursor - start;
        }
    }
    return 0u;
}

#endif
