# Native owned targets and submission lifetime

Windows native DX12 and Vulkan support persistent, single-mip RGBA8 targets.
Enable `native-dx12`, `native-vulkan`, or both through `native`. The `wgpu`
feature remains independent. This API covers owned offscreen targets. Retained rendering and imported
host devices/targets are described in [host interop](host-interop.md).

```rust,no_run
use tileink::{
    Canvas, NativeBackend, NativeContext, NativeContextOptions, NativeRenderer,
};

let context = NativeContext::new(NativeBackend::Dx12, &NativeContextOptions::default())?;
let target = context.create_texture(800, 600)?;
let mut renderer = NativeRenderer::with_context(&context, 800, 600)?;
let canvas = Canvas::new(800, 600, 1.0);

let frame = renderer.render_to_texture(&canvas, &target)?;
frame.wait()?;
// Explicit diagnostic/export readback; ordinary rendering does not copy to CPU.
let image = target.readback()?.readback()?;
# Ok::<(), tileink::NativeError>(())
```

`NativeBackend::Vulkan` uses the same interface. A cloned `NativeContext` refers
to the same logical device; creating another context on the same physical GPU
does not. Rendering rejects a target from another logical device or with an
extent different from `Canvas::physical_size()` before recording GPU work.

Each immediate render replaces the entire target, including clear color and
transparent pixels. Text uses `render_with_text_to_texture`. A cloned target
aliases the same allocation and pixels. Create a new target to change its size;
submitted frames independently retain their allocations through completion.
The renderer's own target, used by `render` and `render_to_image`, also survives
equal-sized frames and is replaced on a successful resized submission.

For asynchronous use, keep submitted receipts and poll `is_complete()`. Polling
neither waits nor consumes readback nor retires resources. Once complete, consume
the receipt with `wait()` (or `NativeImageSubmission::readback()` for an image).
Dropping a receipt does not cancel the GPU work; its context retains pending
resources. Queued frames may safely reuse targets on the context's ordered queue.

First use initializes a new target to transparent pixels. Subsequent explicit
readbacks preserve all channels of its premultiplied RGBA8 contents. No wgpu
fallback, CPU presentation upload, or automatic image readback is involved.
