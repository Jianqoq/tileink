#ifndef TILEINK_HLSL_SCENE_RECORDS_HLSLI_INCLUDED
#define TILEINK_HLSL_SCENE_RECORDS_HLSLI_INCLUDED

// Raw buffer layout, verified against Rust size_of!/offset_of! in GPU ABI tests.
static const uint PATH_RECORD_STRIDE = 76u;
static const uint PATH_SEGMENT_START = 40u;
static const uint SCAN_CHUNK_STRIDE = 16u;
static const uint TILE_SEGMENT_RANGE_STRIDE = 8u;
static const uint SCAN_CHUNK_RANGE_STRIDE = 8u;
static const uint LINE_STRIDE = 24u;
static const uint LINE_P0 = 8u;
static const uint LINE_P1 = 16u;
static const uint PATH_DATA_OFFSET = 16u;
static const uint PATH_BBOX = 24u;
static const uint PATH_TRANSFORM = 52u;
static const uint LINE_SEGMENT_STRIDE = 20u;
static const uint LINE_SEGMENT_Y_EDGE = 16u;
static const uint AFFINE_STRIDE = 24u;
static const uint AFFINE_TRANSLATION = 16u;

#endif // TILEINK_HLSL_SCENE_RECORDS_HLSLI_INCLUDED
