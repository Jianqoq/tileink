# M3 Windows native adapter closeout

Status: Windows minimum native vertical slice implemented; validation receipt is
[`m3-completion-verification.json`](m3-completion-verification.json). The M4 full
Canvas shader inventory and M5 retained/external-target contracts remain separate.
Mac compiler/GPU validation is deferred by the user; MSL remains independent source.
Performance comparison was explicitly waived; none was resumed.

## Module boundaries

The execution implementation now lives in the library, not an integration-test
adapter. Module roots use named files, with no `mod.rs` under `src/native`:

```text
src/native.rs
src/native/runtime.rs
src/native/runtime/
  adapter.rs          shared CommandBatch/UniformWrites bridge and owning receipt
  program.rs          typed dispatch data and pre-recording validation
  submissions.rs      context-bound tickets, attempted/confirmed lease ledger
  pipeline_cache.rs   content framing/locking and driver cache lookup
  dx12.rs             DX12 device, queue, submission, completion and teardown
  dx12/               frame, texture, pipeline, retirement, validation, tests
  vulkan.rs           Vulkan device, queue, submission, completion and teardown
  vulkan/             frame, texture, pipeline, validation, tests
  tests/              independent CPU/WGSL references and shared-adapter tests
```

Both APIs implement the existing shared `BatchAdapter` through `Adapter`.
`CommandBatch` can stage many dispatches and submit them in one native queue call.
Each dispatch currently has a distinct command list/buffer and resources; pooling
and full pipeline scheduling belong to M4. There is no wait in normal submission.
Uniform bytes are resolved and copied before the shared arena clears or reuses a
slot. Abort discards only the unsubmitted suffix and retains the confirmed prefix.

The receipt pins its logical device generation and supports explicit readback even
after the original adapter handle is dropped. Same physical GPU does not make two
logical contexts interchangeable. Foreign encoders, uniforms and receipts fail
before native recording/fence access. An already-consumed receipt fails again.

## Ownership and error semantics

The context registers all frame leases before queue submission. Completion of the
single native submission covers all its command lists/buffers. Reverse readback
does not release other unread batches. Dropping an observation never releases GPU
resources prematurely; healthy last-owner destruction drains with a bounded wait.

DX12 execution followed by a failed Signal is **Unconfirmed**. Vulkan
`OUT_OF_HOST_MEMORY` / `OUT_OF_DEVICE_MEMORY` is **Rejected**: only the attempted
unsubmitted leases are removed, the serial is consumed without reuse, confirmed
prefixes remain valid, and a later batch can retry. Other Vulkan submission errors
stop the context and retain unresolved leases. A failed wait does not trigger a
second teardown wait or authorize release. Unknown completion is quarantined and
reported; this is fail-stop containment, not automatic device-loss recovery.
Host/device import, history reconstruction and full recovery remain M5.

Fault tests inject results at native queue/Signal boundaries after resource
allocation and lease registration. The DX12 fault test actually executes work
before withholding its Signal. Vulkan OOM tests use real pending GPU prefixes and
verify readback/retry after rejection. They do not deliberately exhaust VRAM or
claim that the physical GPU was removed. Unconfirmed-failure tests are isolated
in child processes so retained objects and validation messages cannot contaminate
normal tests.

## Texture and numerical contract

The minimum ABI has buffers at bindings 0/1, the 32-byte parameter block at 2, and
an RGBA8 UNORM sampled Texture2D at 3. DXIL resource kind/register/count and SPIR-V
image type/dimension/array/sample/storage properties are checked. Texture upload
owns row-padded staging on DX12 and explicitly transitions transfer writes to
compute reads on Vulkan. Sampling uses explicit texture loads and interpolation;
fixed-function filtered sampler precision is deliberately not part of the common
algorithm.

Expanded 0.1-coordinate probes reproduced differing final channels with floating
multiply/add contraction across compilation routes. The root fix defines common
Q16 coordinates, decoded from binary32 bits with ties away from zero, followed by
integer linear interpolation and half-up RGBA8 quantization. No tolerance or
fixture-specific adjustment is used. The independent CPU oracle uses f64 rounding
and i64/f64 arithmetic, not the shader bit-decoder. Original dyadic M2 probe
outputs retain their values. This establishes a minimum-program rule; it does not
silently change any production WGSL or certify unported M4 numerical algorithms.

Validation rejects non-finite inputs, signed-coordinate/intermediate overflow,
unknown programs, invalid buffer sizes/alignments, out-of-range dispatch counts,
invalid texture widths and incomplete uniform data before recording. The symmetric
signed coordinate range excludes i32::MIN so negation cannot overflow.

306 cases cover zero/1/63/64/65/129 invocation tails, clear/copy/layout guards,
buffer and real texture sampling, widths 1/17/256 (all 256 channel values),
clamp edges, cancellation, positive/negative steps, quantization tie neighbors,
signed zero, tiny values and non-binary 0.1 coordinates. Each is checked against
the CPU oracle on all four actual APIs for three repetitions: 3,672 route outputs,
zero differing pixels/channels, including transparent bytes and untouched guards.
The queued native test submits all 306 batches before reverse readback.

## Reproduction

Use the explicit DXC, physical GPU LUID, SDK DXC library and Vulkan validation
layer settings in [M2 toolchain documentation](m2-windows-shaders.md). Then:

```powershell
# Optional machine-readable per-case hashes and exact comparison record.
$env:TILEINK_NATIVE_GPU_REPORT = 'G:\Code\tileink\target\m3-four-api-report.json'
cargo test --release --features native --lib native::runtime -- --include-ignored --test-threads=1 --nocapture
cargo test --release --features native -- --test-threads=1
cargo clippy --release --all-targets --features native -- -D warnings
cargo fmt --all --check
```

These are minimum-program native tests. Full SVG/example regressions still execute
the existing wgpu renderer until M4 is ported. `NativeRenderer::new` continues to
report unavailable because the complete Canvas renderer has not been implemented;
passing M3 does not expose a partial renderer as if all drawing operations worked.

Shader artifacts remain persistent and embedded at build time; runtime does not
invoke DXC. Native pipeline caches retain driver/device/layout/shader identity and
crash-safe records. The texture layout revision invalidates older pipeline blobs.
The delivery receipt records tests, package/feature checks, exact pixel reports,
review results and remaining platform scope.
