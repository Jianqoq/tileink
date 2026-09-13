# M3 Windows queued native submission slice

Status: **minimum native execution/lifetime validation extended; M3 is not complete**.
The M2 toolchain/cache delivery `e8631bf52f48752ec9df2c3cd896090bb0e96e65` has
been pushed and the remote branch was verified. This slice continues Windows work;
Metal compiler/GPU validation remains deferred because no Mac is available.

## Problem and resulting behavior

The M2 verification adapters submitted one case and immediately waited for its
readback. That demonstrated shader bytes but could not detect reuse of command
allocators, descriptors, uniform staging or output buffers across in-flight cases.

Native DX12 and Vulkan verification adapters now expose separate `submit` and
`readback` operations. A submit records and queues one complete probe batch, with
no explicit CPU wait for GPU completion. It returns an opaque ticket containing
a logical context identity and monotonic submission number. A ticket contains no
GPU lease: dropping it cannot recycle resources. The context retains each frame
until confirmed completion and explicit readback, or safe context teardown.

`tests/native_shader_gpu/submissions.rs` owns that ledger. Confirmation follows
successful queue/fence submission; an unconfirmed attempt blocks new tracking.
The reserved DX12 `UINT64_MAX` device-removal value is never issued. Foreign
context tickets, unknown/already-consumed tickets, unobserved work and impossible
completion values fail before removing any frame. Identity remains distinct even
when two contexts select the same physical GPU.

This fixes the lifetime limitation directly rather than adding a wait to each
submit. It is a correctness foundation, not a performance acceptance. Allocators,
descriptors and buffers are currently distinct per frame; pooling/reuse after
completion remains later production integration work.

## Native ownership and synchronization

- DX12 frames own their allocator, command list, uploads, parameters, destination
  and readback resources. A private context fence signals each submission number.
  Readback waits for its ticket and retains any other unread frame. Waiting for an
  older frame never marks a still-running newer submission as fully retired.
- Vulkan frames own command and descriptor pools, a fence, buffers and memory.
  Shader-write to host-read visibility is encoded before signaling completion.
  Command/descriptor pools are not reset while an earlier frame can use them.
- Both APIs validate ticket identity before consulting a GPU fence, retain the
  complete batch before queue submission, and remove it only after completion.
  Frame construction failures release only unsubmitted resources.
- DX12 wait/Signal failure retains the whole GPU-owner aggregate until the
  disposable test process exits. Vulkan uses the same containment boundary for
  unknown completion, including the loader and validation callback data.
- Vulkan teardown no longer calls unbounded `device_wait_idle`. An already failed
  context does not wait again. Healthy teardown waits once, for at most 30 seconds,
  on the remaining confirmed fences; failure quarantines resources. This fixes the
  error path where a timed-out readback could hang again during destruction.
  Quarantine also records a validation failure, so cleanup cannot silently pass.

These are executing verification adapters, not yet production `BatchAdapter`
implementations. `NativeRenderer` remains unavailable until shared rendering
integration and its complete semantic contracts are implemented. Test-process
quarantine is not a production device-loss recovery strategy.

## Verification

The queued test submits all 42 cases on both native APIs before requesting any
readback. It checks 42 live entries in each ledger, rejects tickets from the other
context without consuming them, then reads newest-to-oldest and compares all
bytes with independent expected results. It also checks double-consumption and
teardown with an unread final submission. The separate four-API test continues to
compare actual wgpu DX12/Vulkan and native DX12/Vulkan output exactly.

Release CPU tests cover ticket-drop ownership, device identity, unknown completion,
unconfirmed submission, reverse retirement, serial exhaustion and failed-cleanup
wait suppression. Existing isolated DX12 fault tests continue to validate retained
COM fence references and cache-diagnostic boundaries. The Vulkan failure policy
is regression-tested with injected wait outcomes; physical device loss and real
30-second GPU timeouts are not claimed as injected hardware tests.

Run with the same explicit compiler/device/validation-layer settings documented in
[M2 Windows shaders](m2-windows-shaders.md):

```powershell
cargo test --release --features native --test native_shader_gpu -- --ignored --test-threads=1 --nocapture
cargo test --release --features native -- --test-threads=1
cargo clippy --release --all-targets --features native -- -D warnings
```

No production rendering algorithm, shader source or package feature changed in
this slice; no performance comparison was resumed. The minimum queued execution
check is separate from future complete four-API SVG/example/retained acceptance.
Production adapter integration, hardware texture/numerical coverage and real
failure recovery remain M3 work; full shader/effect porting remains M4.

Final validation passed: the nine CPU semantics tests, explicit GPU suite (queued
and four-API cases plus isolated fault checks), full single-threaded native release
suite, formatting and strict all-target Clippy. Both review axes have no remaining
blocking findings. Local source/log receipts are in
[m3-queued-verification.json](m3-queued-verification.json).
