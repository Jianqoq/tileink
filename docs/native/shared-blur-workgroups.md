# Shared blur output workgroups

Metal's `filter/blur_shared.metal` and the HLSL shared-blur entry used by DX12
and Vulkan consume the same validated compact damage list. A filter rectangle
can be much smaller than that list. Previously nonintersecting groups still
loaded their tile/halo and reached the shared-memory barrier, although no lane
could write output. Whole-group rectangle rejection removes this wasted work
at its source, without changing Gaussian weights, taps or rounding.

## Required invariants

- A workgroup origin comes from the group ID or one immutable compact tile ID;
  every lane in the group has the same origin. Reject only when its half-open
  16×16 pixel rectangle has no intersection with the validated output rectangle.
- Reject before any halo load or barrier. In an intersecting group, **all** lanes
  remain active through the barrier, even when individual lanes (including lane
  zero) lie outside the output. Halo lanes may contribute to other lanes' output.
- Existing compact-list bounds, unique tile writers, padded-group handling,
  source-domain checks and transparent halo samples remain required. The host's
  checked extent/radius/padding arithmetic bounds the added tile-end arithmetic.
- Zero deviation and the large-radius global fallback keep their existing output.
  Pixels outside the output rectangle or selected damage tiles remain untouched;
  dispatch ordering, target ownership and retained history are unchanged.

## Real workload and validation

Maximized AAPL Replay on Apple M5, 3420×1966, exactly 780 candles and 120 Hz timed
input. A labeled trace exposes a sigma-3 horizontal blur with an 808×80 output
rectangle but 13,926 active damage tiles, followed by an 808×62 vertical pass
with 13,564 active tiles. The expensive Compute interval also includes clears,
copy, composite and coarse allocation; it is not an isolated coarse kernel time.

Three alternating unprofiled 120 Hz diagnostic pairs improve median mean frame
interval 12.009 → 11.393 ms (5.1%) and render total 11.238 → 10.698 ms (4.8%).
All three mean intervals improve. P95 14.499 → 14.382 ms and worst maximum
16.228 → 17.797 ms do not establish stable tail improvement. Three native 60 Hz
pairs preserve mean cadence, 16.667 → 16.665 ms. The native display query reports
60 Hz; these diagnostics do not certify physical 120 Hz scanout or its 8.333 ms
tail budget. Temporary encoder labels and pacing controls are removed.

One shared physical-GPU semantic test is wired into Metal, DX12 and Vulkan.
It checks an independent f64 Gaussian oracle (one byte rounding tolerance),
exact dense/compact equality and exact untouched history: tiny/odd images,
nonmonotonic tile IDs, wholly rejected groups on all sides, interior partial
rectangles, halo/source boundaries, both axes, zero/fractional deviation,
maximum shared radius and the global fallback. It passes on validated Metal;
DX12/Vulkan physical hardware and compiler validation remain unverified here.

Serial API-validated release all-targets: 902 passes and the same six documented
failures (five coarse glyph/clip oracles, plus 1600-pixel turbulence canonical
mismatch), with three unavailable DXC/historical-WGSL checks excluded. All 1713
SVG PNGs and both blur quality examples match the frozen baseline byte-for-byte.
Examples, native window acceptance and presentation smoke pass. Formatting and
release all-targets Clippy pass with existing warnings; no new test warning.
Two overlap-free labeled GPU trace pairs reduce the filter/coarse cluster mean
1.583 → 1.085 ms and 2.000 → 1.351 ms, about 32%. Whole GPU-window means worsen
in those diagnostic pairs; the cluster gain cannot certify a whole-GPU or tail
gain. Earlier exploratory trace pairs that overlapped XML analysis are excluded.

The full bilingual application report is Northstar's
`docs/replay-pan-blur-workgroups-2026-10-02.md`, including all per-pair timings,
GPU diagnostic records and source/binary identities. This optimization preserves
the general fine/clip scheduling architecture; it does not narrow retained damage
using only current child bounds or change offscreen clearing domains.
