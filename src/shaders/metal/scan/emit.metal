#include <metal_stdlib>
using namespace metal;
#include "config.metal"
#include "../shared/buffer.metal"
#include "geometry.metal"
#include "clip.metal"

kernel void scan_emit(constant ScanConfig& config [[buffer(0)]],const device uint* lines [[buffer(1)]],
    const device uint* paths [[buffer(2)]],device atomic_uint* cursors [[buffer(3)]],device float* segments [[buffer(4)]],
    const device uint* indices [[buffer(5)]],constant BufferSizes& sizes [[buffer(29)]],uint index [[thread_position_in_grid]]) {
    if(index>=config.line_count) return;
    Traversal scan;
    if(!traversal(lines,paths,word_size(sizes,2)/19,scan_index(indices,config.incremental,index,config.line_base),scan)) return;
    for(uint i=scan.begin;i<scan.end;++i) {
        float z=floor(scan.a*float(i)+scan.b);
        int2 tile=traversal_tile(scan,i,z);
        if(traversal_contains(scan,tile)) {
            uint destination=atomic_fetch_add_explicit(cursors+traversal_offset(scan,tile),1u,memory_order_relaxed);
            if(destination<config.segment_capacity) emit_segment(segments,destination,scan,i,z,tile);
        }
    }
}
