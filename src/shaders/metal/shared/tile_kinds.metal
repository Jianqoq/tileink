#ifndef TILEINK_METAL_TILE_KINDS_INCLUDED
#define TILEINK_METAL_TILE_KINDS_INCLUDED
constant uint TILE_KIND_INTERPRETER = 0u;
constant uint TILE_KIND_EMPTY = 1u;
constant uint TILE_KIND_COLOR = 2u;
constant uint TILE_KIND_SDF = 3u;
constant uint TILE_KIND_MIXED = 4u;
// The descriptor stores the kind in its low three bits and a relative raster
// prefix in the remaining bits. Allocation starts remain reusable and unchanged.
constant uint TILE_KIND_MASK = 7u;
constant uint TILE_PREFIX_SHIFT = 3u;
#endif
