const FINE_TILE_KIND_COLOR_ONLY_NO_STACK: u32 = 2u;
const FINE_TILE_KIND_EMPTY_OR_CLEAR: u32 = 1u;
const FINE_TILE_KIND_PURE_SDF_SOLID_NO_STACK: u32 = 3u;
const FINE_TILE_KIND_MIXED_ANALYTIC_SOLID_NO_STACK: u32 = 4u;
const FINE_TILE_LIST_SDF: u32 = 0u;
const FINE_TILE_LIST_MIXED: u32 = 1u;
const FINE_TILE_LIST_FULL: u32 = 2u;

struct FineConfig {
    width: u32,
    height: u32,
    clear_color: u32,
    tile_count: u32,
    tiles_width: u32,
    tiles_height: u32,
    load_target: u32,
    clip_spill_depth: u32,
    group_spill_depth: u32,
    ptcl_capacity: u32,
    paint_sdf_shadow_base: u32,
    paint_brush_base: u32,
    text_image_base: u32,
    text_image_data_base: u32,
    group_spill_base: u32,
    fine_tile_kind_base: u32,
};

@group(0) @binding(0) var<uniform> config: FineConfig;
@group(0) @binding(1) var<storage, read_write> coarse_work: array<u32>;
@group(0) @binding(2) var<storage, read_write> fine_indirect_args: array<atomic<u32>>;

fn fine_tile_kind_at(tile_ix: u32) -> u32 {
    return coarse_work[config.fine_tile_kind_base + tile_ix];
}

fn fine_tile_list_base(list_ix: u32) -> u32 {
    return config.fine_tile_kind_base + config.tile_count + list_ix * config.tile_count;
}

@compute @workgroup_size(1)
fn fine_clear_indirect_main() {
    atomicStore(&fine_indirect_args[0u], 0u);
    atomicStore(&fine_indirect_args[1u], 1u);
    atomicStore(&fine_indirect_args[2u], 1u);
    atomicStore(&fine_indirect_args[3u], 0u);
    atomicStore(&fine_indirect_args[4u], 1u);
    atomicStore(&fine_indirect_args[5u], 1u);
    atomicStore(&fine_indirect_args[6u], 0u);
    atomicStore(&fine_indirect_args[7u], 1u);
    atomicStore(&fine_indirect_args[8u], 1u);
}

@compute @workgroup_size(256)
fn fine_compact_tiles_main(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let tile_ix = global_id.x;
    if (tile_ix >= config.tile_count) {
        return;
    }

    let sdf_base = fine_tile_list_base(FINE_TILE_LIST_SDF);
    let mixed_base = fine_tile_list_base(FINE_TILE_LIST_MIXED);
    let full_base = fine_tile_list_base(FINE_TILE_LIST_FULL);
    let kind = fine_tile_kind_at(tile_ix);
    if (kind == FINE_TILE_KIND_PURE_SDF_SOLID_NO_STACK) {
        let out_ix = atomicAdd(&fine_indirect_args[0u], 1u);
        coarse_work[sdf_base + out_ix] = tile_ix;
    } else if (
        kind == FINE_TILE_KIND_COLOR_ONLY_NO_STACK ||
        kind == FINE_TILE_KIND_MIXED_ANALYTIC_SOLID_NO_STACK
    ) {
        let out_ix = atomicAdd(&fine_indirect_args[3u], 1u);
        coarse_work[mixed_base + out_ix] = tile_ix;
    } else if (kind != FINE_TILE_KIND_EMPTY_OR_CLEAR) {
        let out_ix = atomicAdd(&fine_indirect_args[6u], 1u);
        coarse_work[full_base + out_ix] = tile_ix;
    }
}
