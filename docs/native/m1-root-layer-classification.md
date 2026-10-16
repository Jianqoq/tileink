# Root-layer candidate classification

Root-layer insertion classification now runs only when the change set reports a topology change. Content replacements and affine updates cannot introduce retained layers, so scanning their changed nodes for a new root layer was unnecessary work. This removes that work at its source; it does not defer it to another stage or alter rendering semantics.

The condition follows both transaction and journal-reconciliation semantics. Insertions, layer changes, reparenting and removals mark topology changes. Reconciliation after an expired journal also detects newly introduced layers. Existing layer command locations exclude unchanged layers in conservative change sets.

The journal-gap regression test inserts an opacity layer and child, expires the insertion's journal entry, then compares incremental materialization with a fresh materializer. It checks root command order, node bounds, execution-plan structure and actual child draw membership. A previously published frame remains unchanged. Ordinary layer insertion/deletion tests cover the journal-present path.
