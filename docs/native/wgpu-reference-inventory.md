# WGPU reference program and ABI inventory

This is the reflected M1 shared-renderer candidate snapshot, including the
capacity-independent filter sampler and glass-refraction numerical fixes. It is
not the native HLSL program manifest required by M2. Reflection certifies static
shader layouts and runtime binding remaps; GPU correctness and performance use
separate execution evidence in `NATIVE_BACKEND_PROGRESS.md`.

[`wgpu-reference-inventory.json`](wgpu-reference-inventory.json) contains all 16
WGSL roots, 20 texture-table variants and 179 entrypoint instances. It includes
Naga-validated workgroup sizes, per-entrypoint live resources, logical and runtime
bindings, resource access, type sizes/alignments, struct member offsets, array
strides and image formats. Type IDs are local to each module.
The file is a frozen baseline, not a hand-maintained second source of ABI truth.

[`wgpu-reference-provenance.json`](wgpu-reference-provenance.json) binds this
unchanged reflection file to the frozen pre-cache M1 source snapshot, shader sources and
shared includes, runtime binding and variant code, build and layout inputs,
and the 16 captured WGSL inputs used by reflection. This is static WGPU
provenance; it does not certify GPU execution, performance or native HLSL.

The later module-lifetime cache changes `filter.rs` and adds a benchmark in
`Cargo.toml`; it does not change shader source or ABI.
[`wgpu-filter-cache-provenance.json`](wgpu-filter-cache-provenance.json) records
those two changed reflection-input hashes and the shipping source hashes separately.
The later scoped materializer fix adds its own source delta and CPU benchmark;
its final Cargo manifest hash is recorded without rewriting the frozen cache
source record. It does not change shader source, binding remapping or ABI.
The subsequent materializer order/bounds/input-domain optimizations have a
separate five-file before/after delta in that supplement. They leave the
reflection-input hashes and Cargo manifest unchanged.
The historical provenance above is preserved and is not described as an exact
hash inventory of the final cached implementation.

The audit uses the build expansion list **and** actual runtime constructors in
`buffer.rs`, `scan.rs`, `cumsum.rs`, `coarse.rs`, `fine.rs`, and `filter.rs`.
Range scatter is included even though it uses direct `include_str!` rather than
`build.rs` expansion. Fine/filter each have native/portable texture variants and
atlas-only/texture-table variants. These are all WGPU execution paths.

| WGSL root under `src/wgpu/shaders/` | Table variants | Entry points per variant | Entry points |
| --- | ---: | ---: | --- |
| `scan/clear.wgsl` | 1 | 1 | `scan_clear` |
| `scan/count.wgsl` | 1 | 1 | `scan_count` |
| `scan/prefix_chunks.wgsl` | 1 | 1 | `scan_prefix_chunks` |
| `scan/chunk_offsets.wgsl` | 1 | 1 | `scan_chunk_offsets` |
| `scan/apply_chunk_offsets.wgsl` | 1 | 1 | `scan_apply_chunk_offsets` |
| `scan/emit.wgsl` | 1 | 1 | `scan_emit` |
| `cumsum.wgsl` | 1 | 3 | `cumsum_prefix_chunks`, `cumsum_chunk_offsets`, `cumsum_apply_chunk_offsets` |
| `coarse/count.wgsl` | 1 | 2 | `coarse_count`, `coarse_count_bins` |
| `coarse/prefix.wgsl` | 1 | 11 | `coarse_prefix_chunks`, `coarse_chunk_offsets`, `coarse_apply_chunk_offsets`, `coarse_emit_chunk_counts`, `coarse_emit_prefix_chunks`, `coarse_emit_chunk_offsets`, `coarse_emit_apply_chunk_offsets`, `coarse_emit_fill_refs`, `coarse_emit_chunk_particle_counts`, `coarse_emit_chunk_particle_offsets`, `coarse_tile_counts_from_emit_chunks` |
| `coarse/emit.wgsl` | 1 | 2 | `coarse_emit`, `coarse_emit_bins` |
| `coarse/emit_web.wgsl` | 1 | 2 | `coarse_emit`, `coarse_emit_chunk_tile_kinds` |
| `fine.wgsl` | 2 | 1 | `fine_tile_main` |
| `filter.wgsl` | 2 | 37 | `filter_clear_region`, `filter_copy_region`, `filter_source_alpha_region`, `filter_source_over_region`, `filter_tile_region`, `filter_offset_region`, `filter_turbulence_region`, `filter_flood_region`, `filter_drop_shadow_mask_region`, `filter_morphology_axis_region`, `filter_downsample_region`, `filter_upsample_region`, `filter_upsample_rect_composite_region`, `filter_blur_region`, `filter_blur_shared_region`, `filter_svg_mask_coverage_region`, `filter_color_region`, `filter_color_matrix_region`, `filter_component_transfer_region`, `filter_blend_region`, `filter_composite_inputs_region`, `filter_displacement_map_region`, `filter_convolve_matrix_region`, `filter_lighting_region`, `filter_liquid_glass_region`, `filter_liquid_glass_rect_composite_region`, `filter_composite_drop_shadow_region`, `filter_layer_mask_region`, `filter_rect_mask_region`, `filter_path_mask_region`, `filter_composite_direct_region`, `filter_composite_rect_direct_region`, `filter_composite_stack_region`, `filter_composite_blend_stack_region`, `filter_composite_surface_direct_region`, `filter_composite_surface_stack_region`, `filter_apply_region_mask` |
| `fine_web.wgsl` | 2 | 1 | `fine_tile_main` |
| `filter_web.wgsl` | 2 | 37 | `filter_clear_region`, `filter_copy_region`, `filter_source_alpha_region`, `filter_source_over_region`, `filter_tile_region`, `filter_offset_region`, `filter_turbulence_region`, `filter_flood_region`, `filter_drop_shadow_mask_region`, `filter_morphology_axis_region`, `filter_downsample_region`, `filter_upsample_region`, `filter_upsample_rect_composite_region`, `filter_blur_region`, `filter_blur_shared_region`, `filter_svg_mask_coverage_region`, `filter_color_region`, `filter_color_matrix_region`, `filter_component_transfer_region`, `filter_blend_region`, `filter_composite_inputs_region`, `filter_displacement_map_region`, `filter_convolve_matrix_region`, `filter_lighting_region`, `filter_liquid_glass_region`, `filter_liquid_glass_rect_composite_region`, `filter_composite_drop_shadow_region`, `filter_layer_mask_region`, `filter_rect_mask_region`, `filter_path_mask_region`, `filter_composite_direct_region`, `filter_composite_rect_direct_region`, `filter_composite_stack_region`, `filter_composite_blend_stack_region`, `filter_composite_surface_direct_region`, `filter_composite_surface_stack_region`, `filter_apply_region_mask` |
| `range_scatter.wgsl` | 1 | 1 | `main` |

## Filter bindings and execution

The 37 lazy runtime filter descriptors match the reflected filter entrypoints.
All descriptors add the active-tile resource bit. Their actual group-0 remap
starts with logical bindings 0..3, then the descriptor's used storage resources,
then portable target-read binding 55. The filter-only sampler binding 51 is
absent. Remaining storage bindings and binding 55 fill the rest without duplication. Group 1 image
bindings retain their logical mapping. Both complete 37-program remap sets were
merged into the snapshot; a logical binding number alone is not a native API slot.

| Entry point | Runtime storage resource expression before active tiles | Profile | Shared tile kernel |
| --- | --- | --- | --- |
| `filter_clear_region` | `0` | Clear | false |
| `filter_copy_region` | `0` | Copy | false |
| `filter_source_alpha_region` | `0` | SourceAlpha | false |
| `filter_source_over_region` | `0` | SourceOver | false |
| `filter_tile_region` | `0` | Tile | false |
| `filter_offset_region` | `0` | Offset | false |
| `filter_flood_region` | `FILTER_RES_BRUSH` | Flood | false |
| `filter_drop_shadow_mask_region` | `0` | DropShadowMask | false |
| `filter_morphology_axis_region` | `0` | MorphologyAxis | false |
| `filter_downsample_region` | `0` | Downsample | false |
| `filter_upsample_region` | `0` | Upsample | false |
| `filter_upsample_rect_composite_region` | `0` | UpsampleRectComposite | false |
| `filter_blur_region` | `0` | Blur | false |
| `filter_blur_shared_region` | `0` | Blur | true |
| `filter_svg_mask_coverage_region` | `0` | SvgMaskCoverage | false |
| `filter_apply_region_mask` | `0` | ApplyRegionMask | false |
| `filter_color_region` | `0` | ColorFilter | false |
| `filter_color_matrix_region` | `0` | ColorMatrix | false |
| `filter_component_transfer_region` | `FILTER_RES_TRANSFER` | ComponentTransfer | false |
| `filter_convolve_matrix_region` | `FILTER_RES_CONVOLVE` | ConvolveMatrix | false |
| `filter_lighting_region` | `0` | Lighting | false |
| `filter_liquid_glass_region` | `0` | LiquidGlass | false |
| `filter_liquid_glass_rect_composite_region` | `0` | LiquidGlassRectComposite | false |
| `filter_blend_region` | `0` | Blend | false |
| `filter_composite_inputs_region` | `0` | CompositeInputs | false |
| `filter_displacement_map_region` | `0` | DisplacementMap | false |
| `filter_turbulence_region` | `FILTER_RES_TURBULENCE` | Turbulence | false |
| `filter_composite_drop_shadow_region` | `FILTER_RES_BRUSH` | CompositeDropShadow | false |
| `filter_layer_mask_region` | `FILTER_RES_SCENE_ALPHA` | LayerMask | false |
| `filter_rect_mask_region` | `0` | RectMask | false |
| `filter_path_mask_region` | `FILTER_RES_PATH_MASK` | PathMask | false |
| `filter_composite_direct_region` | `0` | CompositeDirect | false |
| `filter_composite_rect_direct_region` | `0` | CompositeRectDirect | false |
| `filter_composite_stack_region` | `FILTER_RES_SCENE_STACK` | CompositeStack | false |
| `filter_composite_blend_stack_region` | `FILTER_RES_SCENE_STACK` | CompositeBlendStack | false |
| `filter_composite_surface_direct_region` | `0` | CompositeSurfaceDirect | false |
| `filter_composite_surface_stack_region` | `FILTER_RES_SCENE_STACK` | CompositeSurfaceStack | false |

## Capability and test mapping

- Scan, cumsum, coarse and range scatter: storage buffers, integer counts and
  offsets, dense/sparse worklists, bounded dispatch and partial-tail guards. The
  existing semantic suites and `range_scatter`/`root_batches` Criterion scenarios
  remain required alongside final-pixel comparisons.
- Fine: paths/SDF/glyphs/images/gradients, painter order, clip and blend. Native
  texture mode requires RGBA8 storage read/write support; portable mode uses the
  existing read/copy/write sequence. Texture tables additionally require binding
  arrays and sampled-resource nonuniform indexing. The three-mode smoke,
  full 1712-SVG/45-example reference and numeric/coverage/gradient regressions cover
  final output; pipeline counters do not prove every program variant executed.
- Filter/layer/backdrop: source/aux sampling, RGBA8 targets, per-kernel storage
  resources and active tiles. Existing filter semantic suites, full SVG/examples,
  blur/lighting/turbulence regressions and retained history sequences all remain
  required. The shared blur entrypoint uses 16x16 invocations; scalar prefix
  entrypoints and other workgroup exceptions are explicitly recorded in JSON.
- Forced precompiled fine DXIL additionally requires passthrough and the complete
  64-entry texture-array contract. The build provenance manifest records actual
  compiler files, command flags, artifact digests and source fingerprint; it does
  not certify the future maintained HLSL path.

Rust layout facts remain in `src/shared/gpu_layout.rs`, `gpu_types.rs` and their
semantic tests. M2 must replace this frozen audit with the maintained shared
program/ABI description, DXIL/SPIR-V reflection checks and sentinel round trips.
No HLSL/native implementation is counted as complete by this inventory.

The reproducible M1 audit is preserved in
`target/backend-parity/m1-glass-final-1/shader-inventory-1`; its build-output
expansions and dependency libraries come from the frozen `verification-tools-1`
release build. The audit checks actual runtime source before exporting all 20
modules and 74 remap sets. The V13 M0 snapshot remains preserved separately in
`target/backend-parity/m0-shader-inventory-v13`. Hardware capabilities, LUID,
driver, compiler and executed pipeline counts belong to each parity run's
immutable manifest and pipeline report, rather than this static inventory.

## M1 filter sampling and numerical contracts

Source and auxiliary inputs remain sampled textures, declared non-filterable
because the shader uses four `textureLoad` taps in logical texel coordinates.
There is no filter-only logical sampler binding 51 or compact runtime slot.
Group 1's image-atlas sampler remains in use. Entry-point names and workgroup
sizes are unchanged by the sampling and refraction fixes.

Shared layout tests validate sampler-free source/aux bindings; renderer tests
validate logical-edge clamping, premultiplied interpolation and independence from
pooled capacity. Glass regressions invoke the production functions and check
cross-API words, independent geometric expectations and real scene pixels.
The numerical contracts are documented in the GPU pipeline guide. This snapshot
is input to M2 ABI work, not a substitute for the maintained HLSL program manifest,
DXIL/SPIR-V reflection or native-backend sentinel tests.


The accepted scoped leaf follow-up adds a separate nine-file before/after delta
in `wgpu-filter-cache-provenance.json`. It preserves all earlier reflection and
materializer records and binds the final source changes to the complete scoped
CPU/pixel acceptance record. The two new files are private damage-buffer and
chunk-canvas modules; shader source, binding remapping and ABI remain unchanged.


The accepted uniform aggregation follow-up is recorded as a separate two-file
before/after delta in `wgpu-filter-cache-provenance.json`. It changes CPU upload
aggregation and its regression tests, preserving shader source, GPU bindings and
the historical reflection record. Its local Criterion control uses correct
allocation identities; it is not a whole-renderer M0 baseline.
