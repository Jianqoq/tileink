# Native scratch capacity during resize

Internal full-canvas scratch textures keep a logical viewport separate from their
allocation extent. Previously, every changed viewport replaced these textures;
DX12 system sampling showed committed-resource residency and GPU virtual-address
retirement waits during Replay resize. Reusing capacity fixes that allocation
churn at its source. It does not remove presentation or window-message latency.

Only `Targets::acquire` opts into spare capacity. Root outputs and filter-local
outputs retain exact extents, preserving readback and image-sampling contracts.
The two allocation contracts use separate pools so exact roots cannot consume
and discard the spare capacity of scratch leases from the preceding frame.
Scratch growth reserves up to 1.5 times the previous axis, bounded by the adapter's
image-dimension limit. A shrink below one third of either allocated axis returns
excess storage. Invalid requested extents still reach normal context validation.

Every lease clears its entire physical allocation. Subsequent dispatch, clipping
and sampling use the logical viewport; spare texels are not part of the scene.
Memory accounting includes physical capacity. Sibling batches cannot alias live
leases, and retained history pins its allocation. Reuse relies on the existing
single-queue submission order and introduces no CPU fence wait.

The GPU regression `resizing_scratch_reuses_capacity_without_changing_logical_bounds`
checks allocation identity after growth/shrink, logical bounds, physical memory
accounting, and clearing of nonzero pixels across the complete physical extent.
`exact_roots_cannot_consume_scratch_capacity_during_resize` interleaves owned roots
and scratch leases across changing viewports. The existing surface-pool regressions
cover sibling leases, abandoned batches and pinned retained history.

The `native_target_capacity_dx12` and `native_target_capacity_vulkan` Criterion
targets include `native_resize_scratch_capacity`: isolated full-canvas layers at
nearby changing widths, including GPU completion. Real window performance is
measured separately in the trading application's Replay resize scenario.
