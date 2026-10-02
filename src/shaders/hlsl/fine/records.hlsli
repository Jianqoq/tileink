#ifndef TILEINK_HLSL_FINE_RECORDS_INCLUDED
#define TILEINK_HLSL_FINE_RECORDS_INCLUDED
#include "inputs.hlsli"
#include "../draw_records.hlsli"
#include "../coarse_records.hlsli"
#include "../shared/affine.hlsli"

struct FineDraw { uint brush_offset,sdf_offset,shadow_offset; int4 pixel_bounds; AffineRecord inverse_transform; };
FineDraw load_fine_draw(ByteAddressBuffer draws,uint index) {
    uint base=index*DRAW_RECORD_STRIDE;
    FineDraw draw;
    draw.brush_offset=draws.Load(base+DRAW_BRUSH_OFFSET);
    draw.sdf_offset=draws.Load(base+DRAW_SDF);
    draw.shadow_offset=draws.Load(base+DRAW_SHADOW);
    draw.pixel_bounds=asint(draws.Load4(base+DRAW_PIXEL_BOUNDS));
    draw.inverse_transform=load_affine(draws,base+DRAW_INVERSE_TRANSFORM);
    return draw;
}
struct FineTile { uint ptcl_start,ptcl_end; };
FineTile coarse_load_tile(FineInputs input,uint index) {
    uint2 span=input.coarse.Load2(index*COARSE_TILE_RECORD_STRIDE+COARSE_TILE_PTCL_START);
    FineTile tile; tile.ptcl_start=span.x;tile.ptcl_end=span.y;return tile;
}
struct FineParticle { uint tag; int backdrop; uint fill_rule,segment_start,segment_end,color; };
FineParticle coarse_load_ptcl(FineInputs input,uint index) {
    uint base=input.config.tile_count*COARSE_TILE_RECORD_STRIDE+index*COARSE_PTCL_RECORD_STRIDE;
    uint4 a=input.coarse.Load4(base);uint2 b=input.coarse.Load2(base+16u);
    FineParticle particle; particle.tag=a.x;particle.backdrop=asint(a.y);particle.fill_rule=a.z;
    particle.segment_start=a.w;particle.segment_end=b.x;particle.color=b.y;return particle;
}
uint coarse_load_glyph(FineInputs input,uint index) {
    return input.coarse.Load(input.config.tile_count*COARSE_TILE_RECORD_STRIDE+input.config.ptcl_capacity*COARSE_PTCL_RECORD_STRIDE+index*4u);
}
struct FineGlyph { uint image_id; int x,y; };
FineGlyph glyph_at(FineInputs input,uint index) {
    uint3 words=input.text.Load3(index*GLYPH_RECORD_STRIDE);
    FineGlyph glyph;glyph.image_id=words.x;glyph.x=asint(words.y);glyph.y=asint(words.z);return glyph;
}
struct FineGlyphImage { int left,top; uint width,height,content,data_offset; };
FineGlyphImage glyph_image_at(FineInputs input,uint index) {
    uint base=input.config.text_image_base*4u+index*GLYPH_IMAGE_STRIDE;
    uint4 a=input.text.Load4(base);uint2 b=input.text.Load2(base+16u);
    FineGlyphImage image;image.left=asint(a.x);image.top=asint(a.y);image.width=a.z;image.height=a.w;
    image.content=b.x;image.data_offset=b.y;return image;
}
uint glyph_image_data_at(FineInputs input,uint index) { return input.text.Load((input.config.text_image_data_base+index)*4u); }

#endif
