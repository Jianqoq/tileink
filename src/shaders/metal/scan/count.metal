#include <metal_stdlib>
using namespace metal;
#include "config.metal"
#include "../shared/buffer.metal"
#include "geometry.metal"

kernel void scan_count(constant ScanConfig& config [[buffer(0)]],const device uint* lines [[buffer(1)]],
    const device uint* paths [[buffer(2)]],device atomic_uint* backdrops [[buffer(3)]],device atomic_uint* counts [[buffer(4)]],
    const device uint* indices [[buffer(5)]],constant BufferSizes& sizes [[buffer(29)]],uint index [[thread_position_in_grid]]) {
    if(index>=config.line_count) return;
    Traversal scan;
    if(!traversal(lines,paths,word_size(sizes,2)/19,scan_index(indices,config.incremental,index,config.line_base),scan)) return;
    uint delta=scan.down?0xffffffffu:1u;
    for(int y=scan.row_begin;y<scan.row_end;++y) {
        uint offset=scan.backdrop+uint(y-int(scan.bounds.y))*(scan.bounds.z-scan.bounds.x);
        atomic_fetch_add_explicit(backdrops+offset,delta,memory_order_relaxed);
    }
    float previous=floor(scan.a*(float(scan.begin)-1.0f)+scan.b);
    for(uint i=scan.begin;i<scan.end;++i) {
        float z=floor(scan.a*float(i)+scan.b);
        int2 tile=traversal_tile(scan,i,z);
        if(traversal_contains(scan,tile)) {
            bool top=i==0?abs(scan.y0-scan.points.y*(1.0f/16.0f))<=1.0e-6f:previous==z;
            if(top && tile.x+1<int(scan.bounds.z)) {
                int x=max(tile.x+1,int(scan.bounds.x));
                uint offset=scan.backdrop+uint(tile.y-int(scan.bounds.y))*(scan.bounds.z-scan.bounds.x)+uint(x-int(scan.bounds.x));
                atomic_fetch_add_explicit(backdrops+offset,delta,memory_order_relaxed);
            }
            atomic_fetch_add_explicit(counts+traversal_offset(scan,tile),1u,memory_order_relaxed);
        }
        previous=z;
    }
}
