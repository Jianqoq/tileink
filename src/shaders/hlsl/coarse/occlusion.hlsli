#ifndef TILEINK_HLSL_COARSE_OCCLUSION_INCLUDED
#define TILEINK_HLSL_COARSE_OCCLUSION_INCLUDED

#include "config.hlsli"
#include "../shared/tile_kinds.hlsli"
#include "../coarse_records.hlsli"
#include "../shared/particle_tags.hlsli"

// Return a raster-only offset. Never change physical allocation starts: retained
// clips reuse those ranges when emitting later batches and frames.
// PTCL_COLOR is an existing full-coverage proof, never a bounds approximation.
uint occluded_particle_prefix(RWByteAddressBuffer work, ConstantBuffer<CoarseConfig> settings,
    uint tile, uint end, uint wrappers) {
    uint record = tile * COARSE_TILE_RECORD_STRIDE;
    uint start = work.Load(record + COARSE_TILE_PTCL_START);
    if (wrappers != 0u || end <= start || end - start <= 2u
        || end > settings.ptcl_capacity || end > work.Load(record + COARSE_TILE_PTCL_END)) return 0u;
    uint particles = settings.tile_count * COARSE_TILE_RECORD_STRIDE;
    if (work.Load(particles + (end - 1u) * COARSE_PTCL_RECORD_STRIDE) != PTCL_END) return 0u;
    uint depth = 0u;
    for (uint cursor = end - 1u; cursor > start;) {
        uint base = particles + --cursor * COARSE_PTCL_RECORD_STRIDE;
        uint tag = work.Load(base);
        if (tag == PTCL_END_CLIP || tag == PTCL_END_OPACITY || tag == PTCL_END_BLEND) depth++;
        else if (tag == PTCL_BEGIN_CLIP || tag == PTCL_BEGIN_SDF_CLIP
            || tag == PTCL_BEGIN_OPACITY || tag == PTCL_BEGIN_BLEND) {
            if (depth == 0u) return 0u;
            depth--;
        } else if (tag == PTCL_COLOR && depth == 0u && (work.Load(base + 20u) >> 24u) == 255u) {
            return cursor - start;
        }
    }
    return 0u;
}

#endif
