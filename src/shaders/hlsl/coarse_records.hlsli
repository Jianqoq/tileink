#pragma once
// Raw buffer layout, verified against Rust size_of!/offset_of! in GPU ABI tests.
static const uint COARSE_TILE_RECORD_STRIDE = 24u;
static const uint COARSE_TILE_GLYPH_COUNT = 12u;
static const uint COARSE_TILE_PTCL_START = 4u;
static const uint COARSE_TILE_GLYPH_START = 16u;
static const uint COARSE_CHUNK_RECORD_STRIDE = 16u;
static const uint COARSE_CHUNK_GLYPH_TOTAL = 8u;
static const uint COARSE_CHUNK_PTCL_OFFSET = 4u;
static const uint COARSE_CHUNK_GLYPH_OFFSET = 12u;
static const uint COARSE_PTCL_RECORD_STRIDE = 24u;
static const uint COARSE_TILE_DRAW_RECORD_STRIDE = 8u;
static const uint COARSE_TILE_EMIT_RECORD_STRIDE = 8u;
static const uint COARSE_EMIT_RECORD_STRIDE = 28u;
static const uint COARSE_TILE_DRAW_COUNT = 4u;
static const uint COARSE_EMIT_CLASS_FLAGS = 24u;
