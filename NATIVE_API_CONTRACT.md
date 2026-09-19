# Native context, target and completion contract

## Current implementation boundary

The minimum Windows runtime implements shared `BatchAdapter` dispatch/uniform
staging, owning context-generation receipts, bounded readback/teardown and strict
pre-recording validation. API-specific device, pipeline, texture and frame code is
separate under `src/native/runtime/dx12/` and `vulkan/`; named `.rs` files are the
module roots. See [M3 closeout](docs/native/m3-completion.md) for the executable
checks and Q16 numerical contract. Windows owned public contexts and immediate
renderers are now available; see [public renderer contract](docs/native/m4-public-renderer.md).
M4 corpus acceptance is complete. Windows M5 implements owned/imported targets,
retained history, incremental uploads, explicit per-use synchronization and native
window presentation examples. See [M5 implementation](docs/native/m5-implementation.md)
and [host integration](docs/native/host-interop.md) for executable coverage.
Device loss is terminal for a context: fail-stop quarantine protects pending
resources, and the host reconstructs the context/renderer. Automatic device
recreation is not provided. Mac compilation and real GPU validation remain deferred.
Performance comparisons were stopped at the user's request.

## Context and ownership

`NativeBackend` selects one API explicitly. Owned construction creates the device,
render queue and retirement owner for that API. Adapter selection records physical
identity separately from logical device identity: two devices created on the same
GPU are not interchangeable. A renderer created from an existing context shares its
immutable device capabilities and queue owner, while keeping independent scene,
materializer, damage, cache and history state.

API-specific import lives in `tileink::native_interop::dx12` and
`tileink::native_interop::vulkan`. Imports do not transfer responsibility for destroying
caller-owned devices, queues or target allocations. They retain ownership pins for
the device and allocation, including memory and allocator ownership behind a Vulkan
image. Pins must remain alive through the last GPU use, even after the renderer,
registered target and returned completion token have been dropped.

A bare Rust lifetime or a cloned ash function table is insufficient proof of
native resource lifetime. Vulkan import is unsafe: the caller guarantees handle
provenance, enabled features, allocation binding, usage/format metadata and no
premature destruction. The safe wrapper then checks its registered context identity
on every use. It must not claim it can query an arbitrary `VkImage` to prove its
owning device. DX12 import additionally checks the resource/queue device identities
through their native interfaces.

The context owns one queue serialization boundary. Imported Vulkan queues must use
the same host synchronization mechanism for Tileink submissions and application
queue operations. Merely locking Tileink's calls is insufficient when the host also
submits or presents on that queue. The import safety contract covers all such
accesses; it does not require a full GPU wait. Vulkan's normal queue submission
requires externally synchronized host access, and submission order alone does not
establish all needed execution and memory dependencies.
[Queue submission contract](https://docs.vulkan.org/refpages/latest/refpages/source/vkQueueSubmit.html).

## Registered target and per-use synchronization

Registration binds an API-specific allocation to a live context generation. It
records format, extent, dimension, sample count, mip/layer range and allowed uses.
The initial output contract matches WGPU: one 2D RGBA8 UNORM image, one mip, one
layer, one sample, large enough for the physical scene. The scene's output rectangle
is explicit; larger compatible allocations do not imply a different scale or color
conversion. Required storage/read/copy uses come from the chosen verified execution
path. Missing uses or unsupported formats are errors, not implicit conversion.

A registration is allocation identity, not a permanent resource-state assertion.
Each render use supplies its actual incoming state, desired outgoing state and
external synchronization. DX12 specifies typed resource states and fence/value
waits or signals. Vulkan specifies typed image layout, stage/access scopes,
queue-family ownership and semaphore/value waits or signals. Binary and timeline
semaphores remain distinct in the Vulkan interop interface. Swapchain acquire and
present stay in the host; no surface type enters the shared executor.

The caller arranges a matching release before a Vulkan ownership-transfer acquire.
Tileink encodes its matching acquire/release barriers and dependencies; it cannot
manufacture a release from another queue. A same-queue dependency still needs the
resource visibility and state transitions required by the actual accesses.
Unsupported ownership or synchronization combinations fail explicitly before
submitting the target's work. The safe API consumes or exclusively borrows each
per-use descriptor so the caller cannot accidentally reuse a consumed binary
semaphore declaration through a copied value. Unsafe raw-handle aliases remain the
importer's responsibility.

All metadata/context checks that can be resolved before scene work run before
submission. Checks depending on prepared scene capabilities run before any target
dispatch/copy and before history commits. Rejection never returns an apparently
successful completion token. A valid registered allocation does not authorize
unsynchronized external mutation while Tileink is using it.

## History and rendering

Shared output state receives transient or persistent content semantics.
`ExternalTextureHistoryId` retains its current meaning: a caller-provided content
identity, not a fence, allocation handle or device identity. Persistent history is
associated with the registered target, its context generation, image subresource,
scene dimensions and content identity. An ID collision on a different allocation
cannot validate old pixels. Each swapchain image has its own identity.

Transient output never assumes the acquired destination contains the previous
frame. The shared output policy chooses full direct rendering or copy from valid
internal history according to capabilities and existing hysteresis. Persistent
output may use incremental rendering only after the appropriate prior work is
ordered before the next access. External writes, recreation, resize and device loss
invalidate affected output history. `ForceFull` changes damage coverage without
changing painter order or numerical shader semantics.

`render` and `render_retained` submit work and return an asynchronous completion
receipt; an explicit target variant also reports the requested outgoing-state
contract. Text-enabled forms keep CPU text preparation shared. A no-draw frame
still performs required external waits/signals or state transitions. It may return
an already-complete receipt only when no GPU work or dependency is needed and all
relevant prior uses are already known complete. It must not invent a fresh complete
receipt while the previous output is still in flight.

Scene/history readiness is published only after all submissions for that frame are
confirmed. This means later work on the properly ordered queue may consume it; it
does not mean the CPU can overwrite upload storage or the host can present/read the
target without the required GPU dependency. Failed frames invalidate affected
readiness and history, keep confirmed-prefix information and allow a correct retry.

## Completion and retirement

A completion receipt identifies its context generation and an ordered completion
point. It owns access to a retirement owner that also survives independently of
receipts. Nonblocking polling reports pending, complete or device lost; explicit
waiting and readback expose their wait duration. Dropping a receipt never cancels
submitted work and never frees its resources early.

The adapter registers the submission's allocation leases before making the native
submit call. These include targets, descriptor storage, command allocators/pools,
uniform staging and any dependent child-image resources. They are retired only
after the associated native completion has been observed, or through safe
native-device-loss teardown. Logical pool reuse at a resolved frame boundary does
not authorize reuse of an in-flight mapped range or descriptor.

The existing `BatchAdapter` contract distinguishes:

| Result | GPU-use ownership and recovery |
| --- | --- |
| Confirmed submission | Receipt covers this submission and the earlier confirmed prefix of the same batch. |
| Rejected attempt | The attempted work was not accepted; keep any earlier confirmed prefix alive. |
| Unconfirmed attempt | Work may have been accepted without a usable receipt; retain all attempted leases separately from the older confirmed prefix. |

DX12 executes command lists through a void-returning call and adds a fence signal
through a separate fallible call. Therefore a signal failure after execution is
an unconfirmed attempt, not proof that the lists were rejected. This is the
adapter's conservative mapping of the two native calls.
[ExecuteCommandLists](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12commandqueue-executecommandlists),
[Signal](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12commandqueue-signal).
`GetCompletedValue == UINT64_MAX` means device removal and must be tested before
ordinary greater-than-or-equal completion logic.
[GetCompletedValue](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12fence-getcompletedvalue).

For `vkQueueSubmit`, the documented host/device out-of-memory errors guarantee the
referenced state is unaffected by the failed call; those can be rejected attempts.
Device loss follows the device-loss path. Other ambiguous errors stay unconfirmed
unless their native contract provides a stronger guarantee. A later adapter that
uses `vkQueueSubmit2` must verify that function's contract rather than blindly
reusing the mapping.
[Submission errors](https://docs.vulkan.org/refpages/latest/refpages/source/vkQueueSubmit.html).

The context stops normal recording after an unconfirmed attempt until it obtains
an ordering/completion proof covering that attempt or enters device-loss teardown.
An older confirmed receipt is never that proof. Explicit draining/closing can fail
without discarding the unresolved owner. Last-owner destruction must retain or
safely drain its resources; a timeout or a failed wait is not permission to free
potentially in-flight memory. Any exceptional drain is separately observable.
Tileink rendering and target replacement have no unconditional device-idle or
whole-queue wait. The host controls swapchain retirement; the small window example
uses a host wait on resize/teardown, not inside Tileink rendering.

Readback is an explicit submitted copy plus completion and mapping operation.
It returns tightly packed valid RGBA bytes after removing API-specific row padding.
No hidden readback is added to ordinary production rendering or Criterion frames.

## Required executable checks

The existing CPU batch tests already exercise submission rejection, unconfirmed
attempts, confirmed prefixes and aborts. The following additional API-contract
checks define the context/target adapter acceptance coverage:

- Independent renderers sharing one device retain separate scene/history cursors.
- Same physical GPU with different logical devices rejects cross-device targets;
  destroyed/recreated context generations reject stale registrations and receipts.
- Target metadata rejection happens before target writes or history publication;
  compatible larger targets preserve the documented outside region.
- External clear, image replacement, dimensions, content ID, subresource and
  device-loss transitions invalidate the correct history.
- A no-damage frame preserves pending completion and still honors external
  synchronization/state transitions.
- Dropping target/renderer/token before completion retains every native owner;
  completion retires each lease once, including an early-submitted child prefix.
- Signal failure after DX12 execution leaves the attempted leases unconfirmed;
  Vulkan rejected and lost-device results follow distinct recovery paths.
- DX12's removed-device sentinel and completion-counter exhaustion cannot turn
  pending work into successful completion.
- Same-queue and cross-queue import tests run under native validation, including
  real layout/state transitions, binary semaphore reuse and ownership transfers.
- Readback strips padding without changing any valid channel, and four-route
  comparison checks every pixel, including transparent pixels.

CPU state-machine checks do not certify native synchronization. These tests need
real DX12/Vulkan adapter coverage and the required device matrix before release.
