ByteAddressBuffer active_indices:register(t5,space0);
#include "common.hlsli"
ByteAddressBuffer lines:register(t1,space0);
ByteAddressBuffer path_records:register(t2,space0);
RWByteAddressBuffer backdrops:register(u3,space0);
RWByteAddressBuffer segment_tile_counts:register(u4,space0);
#include "geometry.hlsli"

[numthreads(SCAN_CHUNK_SIZE,1,1)]
void scan_count(uint3 id:SV_DispatchThreadID) {
    if(id.x>=config.line_count) return;
    ScanTraversal scan;
    if(!scan_geometry(dispatched_index(id.x,config.line_base),scan)) return;
    int delta=1;if(scan.down) delta=-1;
    uint ignored;
    for(int y=scan.ymin;y<scan.ymax;y++) {
        uint local=uint((y-int(scan.bbox.y))*int(scan.bbox.z-scan.bbox.x));
        backdrops.InterlockedAdd((scan.data_offset+local)*4u,asuint(delta),ignored);
    }
    float last_z=floor(scan.a*(float(scan.imin)-1.0)+scan.b);
    for(uint i=scan.imin;i<scan.imax;i++) {
        float z=floor(scan.a*float(i)+scan.b);
        int2 tile=scan_tile(scan,i,z);
        if(scan_tile_inside(scan,tile)) {
            bool top=last_z==z;
            if(i==0u) top=abs(scan.y0-scan.points.y*(1.0/float(SCAN_TILE_SIZE)))<=SCAN_EPSILON;
            if(top && tile.x+1<int(scan.bbox.z)) {
                int x=max(tile.x+1,int(scan.bbox.x));
                uint local=uint((tile.y-int(scan.bbox.y))*int(scan.bbox.z-scan.bbox.x)+x-int(scan.bbox.x));
                backdrops.InterlockedAdd((scan.data_offset+local)*4u,asuint(delta),ignored);
            }
            segment_tile_counts.InterlockedAdd(scan_tile_offset(scan,tile)*4u,1u,ignored);
        }
        last_z=z;
    }
}
