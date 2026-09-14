# Bounded retained delta index reuse

An index maps node IDs to patch positions; revision, bounds and deletion payloads
do not change this mapping. Previously every nonempty update allocated a new
HashMap and Rc even for the same single ID and slot.

Empty and singleton maps now reuse previous immutable storage only when the
entire ID-to-slot mapping is equal. Other maps rebuild directly using the prior
expected O(n) algorithm. This makes the reuse check O(1), avoiding a second
linear pass when a large mapping changes near the end. Duplicate IDs retain
the last-slot behavior of HashMap construction; they are not assumed unique.
Each delta still owns distinct nonempty patch payloads and version nodes, and old
snapshots retain their original lookup. No global cache is introduced.

This fixes redundant allocation for stable trivial mappings. An exploratory
unbounded-equality candidate was rejected because late-mismatch index builds
regressed; it is not part of this implementation. The constant-size check is
independent of scene shape or benchmark fixtures.

The permanent retained_frame_delta Criterion benchmark covers repeated affine
and invalidation updates. retained_scale covers actual GPU rendering, including
multi-node revision workloads. Regression tests cover old snapshots, version
links, exact slots, permutations, key additions/removals, duplicates, empty
transitions, and removal patches. Successor performance and full pixel checks
are required; earlier candidate results do not grant acceptance.

This extends the empty-index rule in m1-empty-delta-storage.md. Nonempty patch,
damage and dirty-backdrop payload ownership remains unchanged.
