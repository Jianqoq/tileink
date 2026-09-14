#include "region.hlsli"
#include "blur.hlsli"
ConstantBuffer<FilterConfig> config : register(b0);
Texture2D<float4> source_texture : register(t1);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture : register(u3);
ByteAddressBuffer active_tiles : register(t8);
static const uint SHARED_BLUR_HORIZONTAL_PIXELS=SHARED_BLUR_TILE_HEIGHT*(SHARED_BLUR_TILE_WIDTH+2u*SHARED_BLUR_MAX_RADIUS);
static const uint SHARED_BLUR_VERTICAL_PIXELS=SHARED_BLUR_TILE_WIDTH*(SHARED_BLUR_TILE_HEIGHT+2u*SHARED_BLUR_MAX_RADIUS);
static const uint SHARED_BLUR_PIXEL_COUNT=SHARED_BLUR_HORIZONTAL_PIXELS>SHARED_BLUR_VERTICAL_PIXELS
    ? SHARED_BLUR_HORIZONTAL_PIXELS : SHARED_BLUR_VERTICAL_PIXELS;
groupshared uint shared_blur_pixels[SHARED_BLUR_PIXEL_COUNT];
[numthreads(SHARED_BLUR_TILE_WIDTH,SHARED_BLUR_TILE_HEIGHT,1)]
void filter_blur_shared_region(uint3 local:SV_GroupThreadID,uint3 group:SV_GroupID) {
    float std_dev=max(config.amount,0.0);
    uint2 tile_origin=uint2(config.region_x0,config.region_y0)+group.xy*uint2(SHARED_BLUR_TILE_WIDTH,SHARED_BLUR_TILE_HEIGHT);
    if (config.compact_tiles!=0u) {
        uint index=group.x+group.y*config.dispatch_width;
        if (index>=config.active_tile_count) return;
        uint tile=active_tiles.Load(index*4u);
        tile_origin=uint2(tile%config.tiles_width,tile/config.tiles_width)*uint2(SHARED_BLUR_TILE_WIDTH,SHARED_BLUR_TILE_HEIGHT);
    }
    uint2 xy=tile_origin+local.xy;
    bool in_region=all(xy<uint2(config.width,config.height)) && all(xy>=uint2(config.region_x0,config.region_y0))
        && all(xy<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height));
    if (std_dev<=0.0) {
        if (in_region) target_texture[xy]=source_texture.Load(int3(xy,0));
        return;
    }
    int half_width=blur_half_width(std_dev);
    if (half_width>int(SHARED_BLUR_MAX_RADIUS)) {
        if (in_region) target_texture[xy]=rgba8_to_unorm(filter_blur_pixel(config,source_texture,xy,std_dev));
        return;
    }
    uint radius=uint(half_width);
    bool horizontal=config.blur_axis==0u;
    uint stride=SHARED_BLUR_TILE_WIDTH+(horizontal ? 2u*radius : 0u);
    uint count=stride*(SHARED_BLUR_TILE_HEIGHT+(horizontal ? 0u : 2u*radius));
    uint local_index=local.y*SHARED_BLUR_TILE_WIDTH+local.x;
    int4 bounds=blur_sample_bounds(config);
    for (uint load_index=local_index;load_index<count;load_index+=SHARED_BLUR_TILE_WIDTH*SHARED_BLUR_TILE_HEIGHT) {
        int2 position=int2(tile_origin+uint2(load_index%stride,load_index/stride));
        position-=horizontal ? int2(radius,0) : int2(0,radius);
        uint pixel=0u;
        if (blur_inside(position,bounds)) pixel=unorm_to_rgba8(source_texture.Load(int3(position,0)));
        shared_blur_pixels[load_index]=pixel;
    }
    GroupMemoryBarrierWithGroupSync();
    if (!in_region) return;
    uint center=horizontal ? local.y*stride+radius+local.x : (local.y+radius)*stride+local.x;
    float4 accumulator=blur_channels(shared_blur_pixels[center]);
    float sum=1.0;
    float sigma=max(std_dev,0.0001), two_sigma_sq=2.0*sigma*sigma;
    float weight=exp(-1.0/two_sigma_sq), decay=exp(-2.0/two_sigma_sq), ratio=weight*decay;
    for (uint distance=1u;distance<=radius;++distance) {
        sum+=2.0*weight;
        uint delta=horizontal ? distance : distance*stride;
        accumulator=mad(blur_channels(shared_blur_pixels[center+delta]),weight,accumulator);
        accumulator=mad(blur_channels(shared_blur_pixels[center-delta]),weight,accumulator);
        weight*=ratio;
        ratio*=decay;
    }
    target_texture[xy]=rgba8_to_unorm(blur_pack_average(accumulator,sum));
}
