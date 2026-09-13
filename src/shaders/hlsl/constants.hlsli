#pragma once
// Canonical GPU algorithm constants. Rust and WGSL are generated from this file.
// Keep declarations to uint literals, prior constant names and multiplication.

// Physical tile dimension shared by geometry, fine lanes and damage tiles.
static const uint TILE_SIZE = 16u;
static const uint CUMSUM_CHUNK_SIZE = 256u;
static const uint SCAN_CHUNK_SIZE = 256u;
static const uint RANGE_SCATTER_WORKGROUP_SIZE = 256u;
static const uint COARSE_WORKGROUP_SIZE = 256u;
// One fine interpreter lane per pixel, including spill-buffer addressing.
static const uint FINE_WORKGROUP_SIZE = TILE_SIZE * TILE_SIZE;
static const uint FILTER_WORKGROUP_SIZE = 256u;
// Shared blur follows the renderer's damage-tile grid.
static const uint SHARED_BLUR_TILE_WIDTH = TILE_SIZE;
static const uint SHARED_BLUR_TILE_HEIGHT = TILE_SIZE;
static const uint SHARED_BLUR_MAX_RADIUS = 16u;
