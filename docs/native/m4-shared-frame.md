# M4 shared native frame scheduling

Native immediate frames now enter `render::frame::encode`, the same scheduler as
wgpu. It owns plan selection, scan ordering and the direct-root/recursive decision.
The in-place root draw loop is shared through the minimal DrawBatchAdapter; native
code does not pretend to implement the portable texture ping-pong protocol.

`SceneCache::prepare` returns a consumed PreparedScene that keeps the Canvas,
compiled plan and cache borrowed together until scan. This prevents compiling the
plan twice or marking metadata as uploaded merely because preparation succeeded.
Dropping preparation releases those borrows and cannot poison subsequent frames.
Two regressions first reproduced `record(A) -> prepare(B) -> drop -> record(B)`
and a failed B recording followed by retry. Preparation used to update B's
fingerprint while leaving A's cached plan installed. The cache now takes the old
plan out before preparation and installs a plan only after successful recording;
explicit localized plans follow the same success-only rule. This fixes the cache
association at its source rather than compensating during later draws.

The caller supplies an already-resolved Images pair. Any vector child commands
precede the root in the same ComputeBatch. Native scene scan records its remaining
uploads at the scheduler's scan boundary. Each immediate root is a fresh zero-filled
allocation, so clear_root needs no redundant clear dispatch. Scratch reuse remains
confined to the ordered frame batch. No intermediate readback or extra submission
is introduced.

Immediate frames have no retained damage/history and disable early submission.
Those optional scheduler paths remain unavailable until M5. Submission retirement
continues to belong to the native adapter, never to a synchronous frame-recording
wait.

Tests cover discarded preparation, parent restoration after local scan failure,
foreign image rejection and empty transparent frames after prior group draws.
All three full-frame GPU suites pass exact four-API comparisons (489.15s),
including filters, groups/masks and prepared text. Release (1,041 unit tests plus
integrations), strict native all-target lint, native-only compilation, full SVG and
examples pass. All 3,471 PNGs remain present; only the previously approved turbulence
difference remains. Both standards and specification reviews are closed. No
performance comparison was run. Public renderer assembly and complete immediate
four-renderer SVG/example acceptance still remain for M4.
