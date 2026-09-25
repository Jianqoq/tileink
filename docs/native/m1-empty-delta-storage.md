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

Empty and singleton patch indexes additionally follow the bounded immutable
reuse rule in [Stable retained delta indexes](m1-stable-delta-index.md).

## Raster-only upload deltas

Raster-only materializer updates publish an explicit empty `SceneBufferChanges`,
with reusable plan structure. `None` means the input has no incremental upload
contract, so shared/native preparation must rebuild and upload its full data.
It must not represent unchanged retained scene storage. Output damage still
advances normally for both rectangle invalidation and full invalidation.

Initial or unaccepted GPU storage still requires full initialization, even with
an empty delta. Journal gaps retain metadata reconciliation and the
`full_scene_sync` flag; content edits must continue publishing their actual
ranges. CPU `raster_invalidation_publishes_empty_upload_delta` and native GPU
`raster_invalidation_preserves_pixels_across_content_updates` cover this contract.
This fixes repeated full preparation/upload at the source; it does not suppress
requested redraws or change pixels.
