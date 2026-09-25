#include "config.metal"

// The host supplies the actual pixel count, not the allocation's capacity. A
// compact list assigns one 16x16 tile to each consecutive group of 256 pixels.
bool filter_position(constant FilterConfig& config,const device uint* tiles,uint3 id,thread uint2& xy) {
    uint index=id.x+id.y*config.dispatch_width*256;
    if(index>=config.pixel_count) return false;
    if(config.compact_tiles==0) {
        xy=uint2(config.region_x0+index%config.region_width,config.region_y0+index/config.region_width);
        return true;
    }
    uint list=index/256, lane=index%256;
    if(list>=config.active_tile_count) return false;
    uint tile=tiles[list];
    xy=uint2(tile%config.tiles_width,tile/config.tiles_width)*16+uint2(lane%16,lane/16);
    return all(xy<uint2(config.width,config.height)) && all(xy>=uint2(config.region_x0,config.region_y0)) && all(xy<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height));
}
bool filter_contains(constant FilterConfig& config,int2 xy) {
    return all(xy>=int2(config.region_x0,config.region_y0)) && all(xy<int2(config.region_x0+config.region_width,config.region_y0+config.region_height));
}
