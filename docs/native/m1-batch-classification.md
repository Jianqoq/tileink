# Retained batch classification

The materializer classifies the old chunk's plain-fragment status and stable batch membership during the existing chunk lookup, before the generation check and before rebuilding that chunk. This removes a separate traversal without changing damage or painter ordering. Missing nodes and groups remain irrelevant to the predicate; topology changes begin ineligible.

The later stable-batch branch also no longer traverses changed nodes to classify their new chunks. In that branch, old eligibility and a clean plan are sufficient. Rebuilding a chunk with a different plan fingerprint marks the plan dirty; this includes changes to plain-fragment classification. A successful position patch can clear that flag, but the preceding position-patch branch handles it before reaching this condition. Compaction also takes its own rebuilding branch. Same-scale resize does not change plain-fragment classification, and scale changes return through full rebuilding. This is a branch-local invariant: the plan-dirty flag is not globally monotone.

The change removes duplicate node/chunk hash lookups at their source. It is not a workaround or a movement of timing boundaries. Existing fallbacks still rebuild painter metadata when the invariant does not hold.

Semantic tests compare logical painter order and normalized batch grouping against fresh materialization, while validating each physical batch against its compiled plan. Coverage includes both plain/nonplain transitions, conservative unchanged-generation entries, resize followed by another update, actual draw-arena compaction, and successful position patches mixed with unpatchable replacements. These tests protect the invariant rather than claim an old rendering bug.
