# Retained scenes

`RetainedScene` stores a transaction journal and incremental scene data for applications that update a small part of a large frame. Create the scene with a root `RetainedNodeId`, add child canvases in a transaction, and commit atomically. Use `replace_scene`, `set_transform`, and layer updates in later transactions; the scene tracks generations automatically.

`NativeRenderer::render_retained` submits a frame without readback. Use `render_retained_to_image` for a CPU `Image`, or `render_retained_to_target` / `render_retained_to_texture` for host-owned GPU destinations. The returned submission is explicit: wait or read back when the host needs completion. A renderer keeps output history for incremental redraw; call `invalidate_retained_history` if external state invalidates that history.

See [the retained example](website/docs/examples/retained.md) and `src/retained_scene.rs` for the mutation contract.
