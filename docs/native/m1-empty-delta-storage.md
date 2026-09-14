# Retained delta storage

Each retained frame delta remains a distinct version bridge. Immutable empty
patch, damage, dirty-backdrop and patch-index storage can be shared with the
previous frame when both contents are empty. Nonempty patch and damage payloads are constructed
normally; sharing must never drop a new patch or mutate an older snapshot.

This removes repeated allocations of empty `Rc` payloads. It fixes that recurring
work directly without changing delta pruning, compaction, invalidation bounds,
scoped-damage propagation or skipped-version recovery. It introduces no global
cache and retains no old nonempty payload as an empty replacement.

The single-threaded release regression tests in `delta_storage_tests` exercise
invalidation-only updates, distinct version links, old snapshots, nonempty→empty
and empty→nonempty transitions through the actual materializer.

`cargo bench --release --features bench-internals --bench retained_frame_delta`
measures the original retained workload's 100-node affine and manual-invalidation
scenarios through `RetainedMaterializerBenchmark`. Each iteration applies two
scene mutations and materializer updates, following three warmup frames. Its
throughput counts updates, not total scene nodes. Scene creation is outside the
timed region. No GPU is initialized; full renderer/PNG verification remains a
separate obligation. This benchmark guards CPU allocation work and does not
claim to measure full frame time or window/swapchain resize.

Empty and singleton patch indexes additionally follow the bounded immutable
reuse rule in [Stable retained delta indexes](m1-stable-delta-index.md).
