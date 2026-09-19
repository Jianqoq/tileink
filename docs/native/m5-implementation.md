# M5: retained frames and host targets

Status: in progress. Baseline: `79e6153e` (Windows M4 complete).
The user requested M5 before connecting gfx_ui to native DX12/Vulkan. No gfx_ui
native features are claimed as implemented. Performance comparisons remain waived;
exact pixels, lifetime correctness and actual reuse are required.

## Implementation order and acceptance

1. Submission progress and persistent GPU allocations. Hosts can inspect a fence
   without a CPU wait or consuming image readback. Persistent resources carry
   logical-device identity and remain leased until confirmed GPU completion.
2. Renderer-owned targets and uploads survive successive submissions. Dirty ranges
   update existing buffers; immutable uploads are not resent. Scratch leases and
   offscreen history use the shared policies rather than backend-specific scheduling.
3. Native retained recording uses shared materialization, journal recovery, damage,
   partial filter/backdrop and frame execution. Compare Auto with an independent
   ForceFull renderer on every frame, including removal, empty damage and resize.
4. Host-created devices/queues and transient/persistent targets have explicit
   ownership, state/layout, queue synchronization and history contracts. Validate
   foreign devices, extent/origin, history changes, rejected work and device loss.
5. DX12/Vulkan examples demonstrate target acquisition, rendering, completion and
   presentation. The host owns swapchain and window policy. Run the complete retained
   scenario corpus on both native and both wgpu APIs with zero byte differences.

## Invariants

- No CPU readback/reupload presentation path and no implicit wgpu fallback.
- API-neutral materialization, damage, cache identity and reuse policies stay shared.
- A rejected recording/submission cannot publish new history or resource state.
  Unconfirmed work quarantines resources and prevents unsafe reuse.
- Dropping a renderer or target cannot destroy allocations still referenced by a
  submitted frame. Imported Vulkan ownership must outlive command completion.
- External targets retain only the history explicitly promised by their identity;
  transient swapchain images must not inherit unrelated image contents.
- Polling completion is observational: no wait, map, readback consumption or premature
  resource retirement. The explicit receipt completion/readback path still retires work.
- Native DX12 and Vulkan source stays separated; new Rust modules use named `.rs`
  roots. Shader constants stay in HLSLI; no ABI JSON input or hidden shader includes.

## Evidence

### Persistent target foundation

Implemented: nonblocking `NativeSubmission::is_complete`, same-device owned
`NativeTexture`, explicit texture readback, direct `NativeRenderer` target output,
and reuse of the renderer-owned root across equal-sized frames. Rendering clears
the root at the shared frame boundary before blending; it no longer creates an
intermediate root and copies the entire frame into the persistent output. A resize
publishes its replacement only after the queue accepts the submission.

Native frames retain raw allocation owners without retaining their context through
a cycle. DX12 persistent allocations finish in COMMON; Vulkan allocations finish
in GENERAL. Initialization is selected when native commands are recorded, so CPU
batches prepared before an earlier submission cannot clear that submission's pixels.

Focused GPU evidence: `m5-persistent-copy.log` verifies initial partial writes,
unchanged pixels, queued batches, repeated readback, early handle drop and
copy-source/copy-destination transitions followed by compute writes on both APIs.
`m5-public-renderer.log` verifies exact four-route public rendering, clear-color
changes, replacement with an empty frame, wrong-size/foreign-device rejection and
observational completion polling. Both focused suites pass. Independent standards
and spec reviews of this foundation found no outstanding issue.

Full foundation verification passes: 1,049 ordinary release library tests,
integration suites, default/native feature checks, formatting and strict native
all-target Clippy (`m5-foundation-*.log`). The complete six-route corpora report
1,712 SVGs and 45 example images with zero different pixels. All generated PNG
hashes also match the corresponding M4 outputs; there are no new visual changes.
Reports: `m5-foundation-svg/report.json`, `m5-foundation-examples/report.json`.

This is only the allocation/output foundation. Retained damage, incremental buffer
uploads, retained offscreen history, imported host contexts/targets and
window presentation remain pending; no M5 completion is claimed.

### Scratch allocation reuse

Native public recording now uses the shared `SceneResourcePool` for scratch and
local offscreen allocations. Sibling scenes cannot acquire pending storage from
the same batch. The next submitted/discarded batch boundary enables reuse without
a CPU completion wait, and each new lease explicitly clears its GPU pixels.
These are scratch leases, not retained pixel history; cached retained surfaces
must keep separate leases rather than returning their contents to this pool.

`m5-surface-pool-recovery.log` passes on both APIs: physical allocation reuse,
nonaliasing siblings, ordered in-flight reuse, reverse readback, size changes and
discard-after-recording-error followed by reuse with correct initialization.
`m5-surface-pool-public.log` verifies repeated ordinary/filtered/empty frames on
all four APIs. Release tests, independent native feature checks, formatting and
strict native all-target Clippy pass (`m5-pool-*.log`). Standards and spec reviews
have no outstanding findings after the discard/retry regression was added.

The full six-route SVG (1,712) and example-image (45) corpora also pass with zero
different pixels and unchanged PNG hashes relative to the foundation and M4.
Evidence: `m5-pool-svg/report.json`, `m5-pool-examples/report.json`. No performance
comparison was run, as requested. This completes scratch allocation reuse, not
the retained pixel-history or buffer-upload portions of M5.

Evidence goes under `G:/Code/northstar-trading-app/target/agent-work/m5-*` while work
is in progress. M5 is not complete until all four plan checkboxes and its native
host-interop exit condition are verified. Mac compilation and GPU validation remain
deferred for lack of hardware and are not replaced with a Windows result.
