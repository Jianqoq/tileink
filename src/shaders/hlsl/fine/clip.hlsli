#ifndef TILEINK_HLSL_FINE_CLIP_INCLUDED
#define TILEINK_HLSL_FINE_CLIP_INCLUDED
#include "inputs.hlsli"
#include "constants.hlsli"
#include "../constants.hlsli"

void push_clip(FineInputs input, uint mask,
    uint tile_ix,
    uint lane_ix,
    inout uint depth,
    inout uint stack0,
    inout uint stack1,
    inout uint stack2,
    inout uint stack3) {
    uint d = depth;
    if (d == 0u) {
        stack0 = mask;
        depth = 1u;
    } else if (d == 1u) {
        stack1 = mask;
        depth = 2u;
    } else if (d == 2u) {
        stack2 = mask;
        depth = 3u;
    } else if (d == 3u) {
        stack3 = mask;
        depth = 4u;
    } else {
        uint spill_depth_ix = d - FINE_LOCAL_CLIP_DEPTH;
        if (spill_depth_ix < input.config.clip_spill_depth) {
            uint stack_ix =
                (tile_ix * input.config.clip_spill_depth + spill_depth_ix) * FINE_WORKGROUP_SIZE +
                lane_ix;
            input.spills.Store((stack_ix)*4u,mask);
            depth = d + 1u;
        }
    }
}

#endif
