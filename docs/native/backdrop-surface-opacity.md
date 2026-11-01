# Backdrop surfaces and their own opacity

A backdrop nested inside an ordinary opacity group samples that isolated group's content. It
cannot read an external ancestor target. This is correct for an ancestor opacity boundary, but
cannot implement a single surface that has both backdrop-filter and opacity.

`Canvas::push_backdrop_layer_with_opacity` explicitly captures/filters the parent backdrop before
isolating its own foreground. The rounded sample region clips only the filtered background.
Foreground overflow remains visible; the completed source is faded once and then composited through
outer clips. Ordinary `push_opacity_layer` and pass-through `push_backdrop_layer` keep their contracts.
The retained equivalent is `RetainedLayerDescriptor::BackdropWithOpacity`.

The combined path reuses the existing background filter and unfiltered painter-order history cache.
Neither foreground nor faded pixels enter that cache. A fresh pooled source receives the complete
filtered background and foreground before fading, even under partial root damage. Input history
updates retain the original damage mask; copying clean root pixels would read the previous filtered
surface/foreground back into its own input. Scratch leases and suspended work are restored on errors.

Switching between pass-through and isolated backdrop modes recompiles child clip scopes. Changing
only opacity patches the compiled parameter without rebuilding draw batches. The topology contract
matrix includes all nine retained layer descriptors and empty children. Immediate opacity values
are finite and clamped; retained transactions reject invalid values before committing.

The ordinary path adds no surface allocation. The explicit combined path owns one fresh pooled
composition target; its filter/history remain independently reusable. No performance improvement is
claimed without measurement of the consuming workload.

Native gfx_ui acceptance exercises real DX12 immediate and retained rendering at 1x, 1.25x and 2x:
Gaussian sampling beyond the output clip, rounded coverage, sharp borders/foreground, changing
backgrounds, opacity-mode changes, and movement under otherwise unchanged background history.


## Validation

The complete single-thread release suite, strict release Clippy and all example builds pass.
A clean checkout of 793558cf and the candidate each rendered the same 1,712 SVG fixtures with the
same DX12 environment: every PNG hash matches. Existing ignored on-disk outputs were not treated
as an authoritative baseline. Native gfx_ui tests compare retained nested effects with independent
full repaints, including foreground-only movement, background changes and 0/1 opacity endpoints.
