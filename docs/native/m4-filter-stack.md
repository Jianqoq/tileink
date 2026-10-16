# M4 filter stack composition

The Metal and HLSL kernels implement ordinary, blend and translated-surface
composition through ancestor clips. Path and transformed analytic SDF coverage
share the layer geometry helpers and integer alpha rounding.

`Canvas::can_fuse` gives opacity/blend groups containing offscreen descendants
their own surfaces. Consequently `ExecOp::outer_stack` contains clips only;
fused groups remain in draw-batch stacks for fine rasterization. The old fixed
64-group composite evaluator and overflow-skipping branch are deleted rather
than retained as a second implementation.

`Scene::filter_clip_stack` borrows the owning scene's uploaded layer records and
scan geometry. Composite ranges must come from offscreen `outer_stack` ranges.
Encoding validates logical subranges, resource ownership and signed coordinates.
Over derives mask presence from the optional auxiliary texture, binding its
read-only source to the unused descriptor when absent. Blend requires its mask;
Surface uses signed translation and logical extent. Actual offscreen blending
and mask coverage retain their existing semantics.

Retained fragment recompilation explicitly receives the original ancestor clip
stack. Previously compiling a subtree as a fresh root discarded its clips;
backdrop children also lost the clip inheritance that a full compile supplies.
This permanently fixes the compiler's missing input, without a post-render mask
or a full-scene recompile substitute.

Tests cover clip depths 0, 1, 63, 64, 65 and 128, mixed fused/offscreen groups,
masks, backdrops, local coordinate remapping and retained descriptor/topology/
surface updates. Pinned GPU tests verify deep composition pixels and incremental
filter/backdrop/mask insertion/removal against a fresh renderer. The retained
clip regression fails before the compiler fix. Full validation and measured
application timings are recorded in the
[cleanup audit](cleanup-2026-10.md#phase-36-offscreen-clip-composition).
