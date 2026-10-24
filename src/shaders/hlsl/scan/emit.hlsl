#include "../constants.hlsli"
#include "config.hlsli"
#include "index.hlsli"
#include "geometry.hlsli"
#include "clip.hlsli"

ByteAddressBuffer active_indices:register(t5,space0);
ConstantBuffer<ScanConfig> config : register(b0, space0);
ByteAddressBuffer lines:register(t1,space0);
ByteAddressBuffer path_records:register(t2,space0);
RWByteAddressBuffer segment_tile_cursors:register(u3,space0);
RWByteAddressBuffer segments:register(u4,space0);

[numthreads(SCAN_CHUNK_SIZE,1,1)]
void scan_emit(uint3 id:SV_DispatchThreadID) {
    if(id.x>=config.line_count) return;
    ScanTraversal scan;
    if(!scan_geometry(lines, path_records, dispatched_index(active_indices, config.incremental, id.x,config.line_base),scan)) return;
    for(uint i=scan.imin;i<scan.imax;i++) {
        float z=floor(scan.a*float(i)+scan.b);
        int2 tile=scan_tile(scan,i,z);
        if(scan_tile_inside(scan,tile)) {
            uint dst;
            segment_tile_cursors.InterlockedAdd(scan_tile_offset(scan,tile)*4u,1u,dst);
            if(dst<config.segment_capacity) write_clipped_segment(segments, dst,scan,i,z,tile);
        }
    }
}
