#ifndef TILEINK_TILE_KINDS_INCLUDED
#define TILEINK_TILE_KINDS_INCLUDED
static const uint TILE_KIND_INTERPRETER = 0u;
static const uint TILE_KIND_EMPTY = 1u;
static const uint TILE_KIND_COLOR = 2u;
static const uint TILE_KIND_SDF = 3u;
static const uint TILE_KIND_MIXED = 4u;
// The descriptor stores the kind in its low three bits and a relative raster
// prefix in the remaining bits. Allocation starts remain reusable and unchanged.
static const uint TILE_KIND_MASK = 7u;
static const uint TILE_PREFIX_SHIFT = 3u;
#endif
