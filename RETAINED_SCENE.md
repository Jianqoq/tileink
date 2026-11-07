# Retained scenes

`RetainedScene` stores a transaction journal and incremental scene data for applications that update a small part of a large frame. Create the scene with a root `RetainedNodeId`, add child canvases in a transaction, and commit atomically. Use `replace_scene`, `set_transform`, and layer updates in later transactions; the scene tracks generations automatically.

`NativeRenderer::render_retained` submits a frame without readback. Use `render_retained_to_image` for a CPU `Image`, or `render_retained_to_target` / `render_retained_to_texture` for host-owned GPU destinations. The returned submission is explicit: wait or read back when the host needs completion. A renderer keeps output history for incremental redraw; call `invalidate_retained_history` if external state invalidates that history.

See [the retained example](website/docs/examples/retained.md) and `src/retained_scene.rs` for the mutation contract.

## Opacity and clipping

Use `RetainedLayerDescriptor::OpacityGroup { opacity }` or immediate
`Canvas::push_opacity_group(opacity)` to fade a complete subtree. These isolate
and composite descendant paint once without introducing a layout-sized clip.
The work bounds include overflowing child geometry and nested effect output.
An explicit `ClipPath` / `ClipSdf` remains an independent clipping boundary.
Path-masked `Opacity` / `push_opacity_layer` retain their authored mask semantics,
including their existing single-stack fusion path.

Unmasked groups use one cached source surface and allocate no mask surface. Dirty
updates clear and repaint only active tiles; clean cached output skips child work.
A layer shell has no child content: frame damage, spatial domains, structural
batch reassignment and parameter patches must resolve an unmasked group's actual
installed subtree, rather than treating the empty shell as its output. This fixes
both overflowing controls disappearing on fade and consecutive alpha updates
leaving pixels at the previous opacity. It is a root-cause compositing fix.

The GPU regression `unbounded_opacity_applies_once_preserves_overflow_and_matches_retained_updates`
compares immediate, fresh retained and incrementally updated pixels at 1x/2x,
including overlapping opaque children, nested clipping, child movement and an
explicit outer clip. The CPU regression
`unbounded_opacity_parameter_updates_preserve_the_descendant_output_domain`
locks down consecutive parameter updates without rebuilding draw order.

SVG compatibility is checked against an unmodified build on the same DX12 host:
[all 1,712 native fixtures are byte-identical](docs/native/opacity-groups-svg-check.json).
The stored repository PNG baseline already differs in 722 images on this host;
the same differences reproduce without these changes. Generated PNGs were restored
and the original reference images were not updated.
