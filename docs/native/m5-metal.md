# M5: macOS retained rendering and host presentation

Status: implemented and verified on Apple M2 / macOS 15.0.1 (24A348), using
Xcode 16.2 and the macOS 15.2 SDK. This closes Mac M1–M5 on the recorded device;
the wider M6 hardware/platform matrix remains open. Performance comparisons are
waived by the user's instruction and no performance result is claimed.

## Retained acceptance

`tests/metal_retained_parity.rs` executes the same 29-state scene sequence used by
Windows M5. Backend runners and semantic assertions are extracted into reusable
modules so neither platform maintains a separate approximation of the corpus.
References and native execution are separate, exclusive feature builds on the
same physical GPU. Font bytes/order/locale, scene source hash and frame manifest
must match before native comparison starts.

The matrix contains 18 variants: wgpu-Metal with native and portable textures,
and native Metal; each has independent Auto/ForceFull renderers for owned,
transient and persistent targets. All 522 outputs match exactly, including alpha
and transparent RGB. Independent checks require partial redraw on designated
edits, zero uploads/rebuilds on static frames, complete ForceFull redraws, journal
recovery and resumption, resize without geometry rebuild, and empty output after
removing all content. Inputs include geometry, images, clipped text, layers,
filters/backdrops, masks, reorder/reparent, target replacement, external writes,
history invalidation, old target reuse, DPI changes and 15/16/17-pixel boundaries.

Focused native tests additionally cover output origins and untouched sentinels,
persistent buffer/image/offscreen allocation reuse, discarded recording recovery,
reverse readback and early handle drop. Metal-specific tests reject unsupported
texture formats/usages/mips/hazard tracking and foreign logical-context targets;
rejection leaves a valid renderer usable. An injected failed-context state verifies
that new work and completion retirement are rejected while pending leases remain
owned. This is a deterministic failure-policy test, not physical GPU removal.

## Host presentation

The existing `native_present` example now supports `metal`. The example owns its
NSWindow/NSView and pumps AppKit events directly; the Windows example uses Win32.
This removes the extra window/event-loop dependency while retaining the same host
rendering contract. The host owns CAMetalLayer, a persistent RGBA8 target, two Metal
queues and a shared event. Tileink receives the host device/queue and imported
target. Per-use waits/signals order the host's previous read, Tileink's writes,
and the next host read. Empty-damage frames still perform the handoff.

CAMetalLayer uses BGRA8Unorm; a host-owned fullscreen render pass converts the
RGBA target to the drawable. There is no CPU pixel readback/reupload in presentation.
This small example presentation shader compiles from MSL at host initialization;
Tileink's renderer shaders remain offline metallib artifacts. A dedicated GPU test
checks every output channel (including zero alpha), orientation and edge texels at
widths 1, 17 and 257. It does not rely on a screenshot or composited window colors.

Ordinary render/resize does not wait for GPU completion. Command buffers retain
encoded resources, and pending frames retain Tileink receipts. Retirement observes both host and Tileink completion before releasing receipts;
a controlled unsignaled-event test prevents premature retirement or an implicit
wait in this ordinary frame path. Explicit shutdown uses a bounded wait and preserves unconfirmed owners on failure. Smoke acceptance
requires eight submitted/completed presentation frames and at least two actual
physical window sizes; the measured run resized 640×360 to 480×270. A no-op resize
cannot silently pass. The example is a host integration demonstration, not gfx_ui
integration or a production window policy.

## Reproduction and evidence

```sh
bash scripts/mac/run_native_metal_tests.sh --retained
bash scripts/mac/run_native_metal_tests.sh --present
```

Both enable Metal API validation. Tests are release builds with one test thread.
Retained references and per-frame SHA-256/statistics reports are written under
`target/metal-validation/retained`. Set `TILEINK_NATIVE_GPU` to a registry ID to pin
the host example; the retained runner pins native execution to its reference GPU.
See [Metal verification](metal-verification.json) for the M1–M4 receipt and
[M5 verification](m5-metal-verification.json) for the continuation's report hashes
and final checks. Full SVG/example comparisons and the default wgpu regressions
remain required. Cross-platform checked-in PNGs are not updated by this work.
