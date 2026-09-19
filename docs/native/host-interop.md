# Native host devices, targets and presentation

Enable exactly one of `dx12` or `vulkan`, with default features disabled.
These features are mutually exclusive with each other and with default `wgpu`.
`tileink::native_interop::{dx12,vulkan}` contains typed host
context and texture descriptors. These are unsafe imports: Tileink validates the
queryable device, format, dimensions and usage, while the host must provide
accurate Vulkan handle metadata and synchronize accesses on the imported queue.

## Ownership and target state

DX12 retains COM references. Vulkan descriptors retain an `Rc<dyn Any>` owner;
its destructor owns the host allocation/device destruction. Tileink creates and
destroys its own Vulkan image views but never destroys imported image memory,
logical devices or instances. Raw allocation owners, rather than NativeContext
cycles, keep submitted frame resources alive. Unknown completion quarantines the
whole ownership chain rather than recycling resources.

Targets are one-sample, one-layer, one-mip RGBA8 UNORM images. DX12 requires UAV
and shader-read access (DENY_SHADER_RESOURCE is rejected). Vulkan requires
SAMPLED, STORAGE, TRANSFER_SRC and TRANSFER_DST usages. Backdrop/filter operations
may read the target even when the application primarily considers it an output.
Unsupported host formats should be converted by the host GPU presentation pass.

Texture descriptors provide defaults for ordinary renders on the imported queue.
Explicit host handoffs use a consumed, non-Clone `NativeTargetUse`, constructed by
`NativeContext::dx12_target_use` or `vulkan_target_use`. It supplies the actual
incoming state and desired outgoing state for that use. Renderer entry points
include immediate, retained, and their text-enabled variants. They return a
`NativeTargetSubmission` containing completion and the outgoing state.

DX12 descriptors carry typed resource states and fence/value waits and signals.
Tileink validates device identity, resource flags and state combinations before
submission. Vulkan descriptors carry layout, stage/access scope, queue family,
binary/timeline semaphore waits and signals, and an ownership pin. Timeline use
requires the imported device's timeline feature to be enabled and declared.
Invalid or duplicate semaphore declarations and incompatible scopes are rejected.

Tileink emits Vulkan acquire/release ownership barriers. The host must issue the
matching release/acquire barriers and semaphore dependency on the external queue;
queue-family indices and layout transitions must match. Wait scopes cover all
Tileink transfer and compute commands. Ordinary use cannot access an image still
owned by another family. Initial UNDEFINED is allowed only for an uninitialized
registration; it never permits reading undefined pixels. Reimport after raw host
writes/discard to create a fresh content identity, or change the explicit history
identity when preserving the allocation.

Only an accepted submission publishes outgoing state and retained history. Even
an empty-damage frame performs requested synchronization and ownership handoff.
Failure after queue execution quarantines unconfirmed resources and prevents
continued context reuse; it does not publish successful history.

## Retained output contracts

`render_retained_to_target` accepts `NativeRenderTarget`:

- `From<&NativeTexture>` tracks one persistent allocation and its accepted write
  version. Same-sized output can be rendered directly without a root copy.
- `persistent(texture, ExternalTextureHistoryId)` additionally tracks the host's
  explicit content/history identity. Changing it invalidates retained history.
- `transient(texture)` uses renderer-owned history and copies the completed frame
  to the output. Rotating swapchain images cannot inherit unrelated old pixels.
- `with_origin(x,y)` writes a validated subrectangle and preserves pixels outside
  it. Overflow and out-of-bounds rectangles fail before rendering or history commit.

The older `render_to_texture` interface requires exactly matching extents. Explicit
target rectangles may use larger images. Two renderers have independent retained
state; a write through one NativeTexture handle invalidates another renderer's
tracked output history. Arbitrary raw host writes cannot be detected automatically.

## Presentation and synchronization

The host controls acquire, frame pacing, resize and present. Rendering never reads
pixels back to CPU unless `render_to_image` or explicit texture readback is called.
A `NativeSubmission` supports observational `is_complete` and explicit `wait`.
These concern Tileink work; later host copy/present operations need a host fence.

The runnable Windows `native_present` example separates DX12 and Vulkan modules:

```powershell
cargo run --release --no-default-features --features dx12 --example native_present -- dx12
cargo run --release --no-default-features --features vulkan --example native_present -- vulkan
# Hidden, bounded acquire/render/present/resize smoke verification:
cargo run --release --no-default-features --features dx12 --example native_present -- dx12 --smoke
cargo run --release --no-default-features --features vulkan --example native_present -- vulkan --smoke
```

Both use existing host devices and queues. DX12 renders directly into an imported
host UAV allocation and performs one GPU CopyResource into the DXGI flip buffer.
Its fence is signaled **after Present** and waited before command allocator reuse
or ResizeBuffers. This covers host commands beyond Tileink's own completion fence.
The example's Vulkan surface must advertise RGBA8 UNORM and the required usages;
it rejects unsupported configurations rather than silently changing color semantics.

Vulkan passes the acquire wait and present-finished signal into TargetUse; both
belong to the same native submission as rendering. Present waits on that signal.
Finished semaphores are indexed by swapchain image, preventing reuse while
presentation still consumes them. Host queue access must remain serialized.
The example waits for the host device at resize/teardown and skips zero-sized
windows. Production hosts can retire old swapchains using their own pacing policy.

These scopes follow [Vulkan synchronization](https://docs.vulkan.org/spec/latest/chapters/synchronization.html)
and [vkQueueSubmit](https://docs.vulkan.org/refpages/latest/refpages/source/vkQueueSubmit.html).
DX12 buffer states and queue presentation follow [Microsoft's swapchain contract](https://learn.microsoft.com/en-us/windows/win32/direct3d12/swap-chains).

Mac MSL compilation/GPU verification remains deferred. This integration does not
implement gfx_ui's feature selection or automatic device recreation: a lost device
is terminal for its context, and the host constructs a new context/renderer.
