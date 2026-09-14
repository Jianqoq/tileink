# Root-layer candidate classification

Root-layer insertion classification now runs only when the change set reports a topology change. Content replacements and affine updates cannot introduce retained layers, so scanning their changed nodes for a new root layer was unnecessary work. This removes that work at its source; it does not defer it to another stage or alter rendering semantics.

The condition follows both transaction and journal-reconciliation semantics. Insertions, layer changes, reparenting and removals mark topology changes. Reconciliation after an expired journal also detects newly introduced layers. Existing layer command locations exclude unchanged layers in conservative change sets.

The journal-gap regression test inserts an opacity layer and child, expires the insertion's journal entry, then compares incremental materialization with a fresh materializer. It checks root command order, node bounds, execution-plan structure and actual child draw membership. A previously published frame remains unchanged. Ordinary layer insertion/deletion tests cover the journal-present path.

The original M0 `one-affine-plan-sync/100` benchmark reproduced a roughly 14.1% regression before this change. The isolated proposal's matched four-case forward/reverse and same-version campaign completed 12 processes and 24 comparisons without a current-adverse comparison. Plan-sync and production were statistically unchanged against M0 in both orders; this is not a claim of strict one-percent equivalence. Inclusive materialization stayed in Criterion's noise classification. Original comparison orientations and all self-controls are preserved in the evidence.

This local result does not establish whole-M1 performance acceptance or a window-resize PMax improvement. Final corpus, broader performance and integration checks remain separate gates. The diagnostic allocation probe is not shipping code and its instrumented timings are not performance evidence.
