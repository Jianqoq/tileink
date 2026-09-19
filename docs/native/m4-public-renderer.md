# M4 owned native renderer

The Windows `native-dx12` and `native-vulkan` features now expose an owned
`NativeContext` and `NativeRenderer`. Construction selects the requested API and
optionally an exact physical adapter LUID; an unavailable backend, unmatched LUID,
or insufficient texture-table capacity returns an error without fallback.

`NativeRenderer::with_context` shares device and queue ownership while keeping
scene, image and text preparation caches independent. Image registration and
Canvas image resources use the shared preparation contracts. Text rendering accepts
the same font system and text context as wgpu. Dimensions are checked before
recording against actual device limits.

`render` and `render_with_text` return `NativeSubmission` without an implicit
readback or wait. `wait` explicitly waits for completion. `render_to_image` and
`render_to_image_with_text` request a copy and return `NativeImageSubmission`;
its consuming `readback` returns the original premultiplied RGBA8 bytes without a
second premultiplication. Receipts retain context and resource ownership even
after the renderer or caller's context handle is dropped. Unconfirmed submissions
remain distinct from rejected submissions and quarantine resources when completion
cannot be established. Automatic polling and continuous-frame retirement remain M5.

## Validation and initialization

Validation defaults to off. DX12 debug-layer activation is a process-wide operation:
applications wanting Tileink to activate it must call the unsafe
`NativeContext::enable_dx12_validation` before creating any DX12 device, including
wgpu devices. Its safety contract also excludes concurrent external device creation.
Safe context construction never activates the layer, and rejects validation requests
when no debug queue is available. Owned creation is serialized with layer activation;
a late activation request is rejected before it can invalidate existing devices.

`check_validation` reports diagnostics. It permits only the known wgpu optimized-clear
advisory at warning severity, which remains printed; error severity and unrelated
warnings remain failures. Vulkan validation is enabled per context.

Low-level conformance probe pipelines are initialized only when a probe is submitted.
Production Canvas rendering does not create them. Vulkan probe initialization rolls
back partially created layouts and pipelines on failure so retry is safe.

The no-debug DX12 teardown regression fixes the root cause: an optional diagnostic
queue must never be unwrapped before resources are quarantined. Tests reproduce the
previous panic and check that the real COM fence owner remains pinned afterward.

## Acceptance boundary

The public-entry GPU test compares both native APIs against both wgpu APIs on the
same RTX 4090, with validation enabled and disabled. It covers premultiplied alpha,
independent renderers sharing a context, reverse-order readback, resizing and dropping
renderer/context handles before readback. Focused lifecycle tests cover late debug
activation, quarantine without a debug queue, and lazy probe failure/retry.

This slice does not close M4. Background clearing is implemented in the subsequent
[root clear color slice](m4-clear-color.md); the complete four-renderer SVG/example
corpus remains required. External targets, retained history,
automatic device-loss recovery and Mac hardware validation remain outside this slice.
No performance comparison was run, as requested by the user.
