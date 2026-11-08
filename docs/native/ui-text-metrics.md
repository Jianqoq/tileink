# Shared UI text metrics

High-level `TextContext::layout` and `layout_buffer` use one line-placement rule exposed by
`text_layout_runs`. It borrows the existing shaped buffer and cached paragraph layouts. It does
not allocate another buffer, reshape text, round glyph advances, change line heights, or modify
the caller's Cosmic Text buffer.

For each visual line:

- Round the maximum font ascent and descent in logical UI units.
- Floor `(line_height - ascent - descent) / 2` for the leading above the baseline. The remainder
  stays below, including for fractional or negative leading.
- Add that leading and ascent to the exact line top. Preserve fractional scrolling and spacing.
- Add each glyph's shaping offsets once. Use that same origin for raster and outline placement;
  the existing bounded CanvasGlyph conversion owns subpixel bins and integer carry.

This fixes the root cause of vertically biased UI text: previously fractional font metrics fed a
baseline that was truncated before rasterization, while outline text retained a different origin.
Noto Sans 14px/21px now places both at baseline 16, rather than raster baseline 15. Browser-style
font-metric rounding and upper-leading placement are defined in Chromium's font_metrics.cc and
core/layout/inline/line_utils.cc; no per-font or component compensation is used.

Visible-line filtering uses the resolved font bounds together with the original line box.
Partly visible empty paragraphs remain available to editing geometry. `text_hit` uses those
same lines and shaped glyph advances. Aligned and mixed-direction text select the nearest
physical glyph edge; within a ligature, logical grapheme order follows the glyph direction.
Nonfinite pointer positions have no hit. This replaces the old UI placement/hit path; there
is no compatibility switch retaining the former baseline calculation.

Consumers that need UI baseline coordinates must use `text_layout_runs`, rather than the raw
shaper's `LayoutRun::line_y`. Raw buffers remain the content/shaping source; their fractional
advances and line boxes are valid measurement data. Carets and selection cover those line boxes,
not the font ink bounds. Fractional intrinsic sizes therefore do not accumulate integer errors.

DPI and affine conversion happen later in the renderer. A baseline is not truncated in logical
space before scaling. Raster cache keys retain font identity, size, weight and flags; position
changes reuse images whenever their subpixel phase is unchanged. Explicitly positioned Canvas
and SVG glyphs retain their authored coordinates and do not acquire UI half-leading.

Regression coverage includes actual raster/outline layout, fractional spacing and scrolling,
negative leading, empty paragraphs, wrapping, font fallback, combining offsets, aligned/RTL
hit testing, grapheme clusters, and the retained-buffer/owned-layout equivalence contract.

## Validation

The normal single-thread release suite passed (915 library tests, 195 opt-in tests ignored,
plus integration/example tests). The explicitly run native GPU fractional-glyph-motion and
clipped-text fade tests passed, including 1x/2x DPI and both LCD channel orders. Strict release
Clippy and formatting passed. The complete SVG fixture renderer and all example builds passed;
all 1,712 generated SVG PNGs were byte-identical to the pre-change run on this machine.
The independent gfx_ui_shadcn fixture matches 48 installed-browser baseline/line-height pairs.

## LCD clarity: preserve the resolved area coverage

Harmony rasterization renders three full-pixel antialiased outline masks at the physical
RGB/BGR channel offsets. Their area integration already supplies the equivalent of a light
LCD filter, as described by [FreeType's subpixel-rendering contract](https://freetype.org/freetype2/docs/reference/ft2-lcd_rendering.html).
Merge these channel samples without another spatial convolution. The removed 21/214/21 FIR
pass smoothed an already antialiased result, widened its footprint, allocated two extra pixel
columns, and softened small UI text. This fixes that source of softness at rasterization;
it is not a weight, font-family, gamma, baseline, or component-specific adjustment.

The default emboldening, contrast/compositing policy and real fractional affine interpolation
remain unchanged. Integer-aligned draws still read exact texels; interpolating moving text is
necessary for continuous motion. RGB/BGR ordering, color-emoji fallback and canonical
three-byte LCD image layout also retain their existing contracts. Rasterization algorithms
can differ from a browser: comparable clarity does not require pixel-identical glyphs.

A regression exercises the real raster entry point against independently resolved outline
coverage across serif/sans fonts, weights, sizes, physical scales, fractional pixel phases,
both LCD channel orders, and zero/default emboldening. It failed on the previous extra-filter
footprint before the fix. The native Inter fixture isolates the actual renderer from any
component skin or opacity surface; forcing nearest-neighbor GPU sampling or disabling hinting
was byte-identical to its previous output, ruling those out for this case.

Validation of this change: the full single-thread release suite, strict release Clippy and
formatting checks passed; four explicitly enabled native GPU text tests passed (fractional
motion, transparent/clipped fading, large-coordinate cancellation and SVG font fallback).
All 1,712 SVG renders were byte-identical to a fresh pre-change run on this machine; seven
image-example runs and all example builds passed. The ordinary native Nova/Vega Trigger
interaction captures still match all 12 browser rectangles and the focus-ring bands.

For the isolated Inter 14px/500 light-background line, normalized edge uncertainty
`sum(a*(1-a))/sum(a)` fell from 0.3090 to 0.2982 (Chrome: 0.2774); the excess over that browser
sample fell from 11.4% to 7.5%, while total ink changed by 0.25%. Comparable real 14/16px
light/dark component samples also improved. This is a diagnostic of edge spreading for these
samples, not a universal perceptual sharpness score or a claim that all glyphs match Chrome.

## Windows native masks and one color-transfer rule

Ordinary Windows text uses DirectWrite's natural-symmetric, natural-measuring glyph analysis.
It produces CPU coverage only; Tileink retains ownership of layout, glyph caching, atlas upload,
GPU rendering and color composition. RGB/BGR masks keep three canonical bytes per pixel and
alpha masks keep one. Cache hits do not call DirectWrite again. Physical pixel size, subpixel
phase, pixel-font phase rounding, disabled hinting and synthetic italic remain part of the
existing cache-key contract. Invalid glyph indices return no image, because DirectWrite's
implicit .notdef substitution would violate the portable missing-glyph/cache contract.

A lazily registered in-memory font loader copies each database face once. Native variable-font
instances preserve default axes except the requested weight, clamped to its declared range;
instances are keyed by that resolved weight. Equivalent out-of-range requests must share one
face. Clearing glyph caches after a font-database replacement also drops the copied resources.
The loader is unregistered only after its fonts have been released. No HWND, window renderer,
or second text-layout system is introduced.

Color outlines and bitmap emoji keep their established Swash priority. Explicit positive outline
expansion uses the portable adapter, which implements that operation. Unsupported native fonts
or native initialization errors also use the portable path. Ordinary Windows mask expansion now
defaults to zero so the actual requested font weight reaches DirectWrite; other platforms retain
their existing portable defaults. These are rasterization policies, not component offsets.

The default Linear mode uses a single correcting-coverage transfer and the renderer's common
encoded, premultiplied RGBA source-over. For raw coverage a and straight foreground f, use the
complementary background b=1-f and enhance coverage by `a + a*(1-a)*srgb_to_linear(b)`. Convert
that blend in linear light back to sRGB and solve the equivalent encoded-color coverage. A
near-equal foreground/background uses the continuous coverage limit to avoid an ill-conditioned
inverse. This is Skia's correcting-LUT principle, evaluated continuously rather than quantizing
the foreground into a small set of LUT colors. Explicit Srgb mode keeps raw encoded source-over.

Every texel is corrected before bilinear interpolation; corrected coverage stays floating point
until final color output. Clip and source opacity apply afterward. Unknown/transparent targets
reduce corrected LCD channels to their scalar mean. This ordering preserves total ink as a label
moves fractionally or enters an offscreen opacity group. Correcting an interpolated raw mask
increased light-on-dark ink; blending only the first draw in actual linear light increased ink
when its encoded RGBA layer was later composited. The existing real fade/motion tests caught both
cases. Neither a font-weight adjustment nor a component-specific offset can fix that mismatch.

Cached font masks remain independent of foreground color. Integer-aligned draws still read one
texel; fractional draws interpolate corrected neighboring texels without rerasterization or new
atlas entries. There is no spatial convolution, empirical exponent, apparent-axis correction,
or stem-core boost in the correcting path. Layout, shaping and cache identity stay shared.

Regression coverage exercises the actual glyph cache against an independent file-based
DirectWrite reference across sizes, phases, channel modes, italic, hinting and pixel-font flags;
empty/invalid glyphs, explicit expansion, color emoji and resolved variable-weight reuse are
covered too. Native compute validation compares the real alpha/LCD shaders with a high-precision
color oracle across 10,752 coverage, palette, clipping and transparency requests. The old transfer
failed that GPU test before replacement. Rendered component acceptance remains a separate check.

Validation on Windows: the full single-thread release suite passed (923 library tests plus
integration/doc tests), formatting and strict all-target release Clippy passed. Native GPU
coverage transfer, clipped fading, eighth-pixel motion, large-coordinate cancellation and SVG
font fallback tests passed. All 1,712 SVG fixture PNGs remain byte-identical to the previous
baseline. Nova/Vega component interactions, all 12 Trigger rectangles and their focus bands
still pass their installed-browser comparison.

The real Inter 14px/500 light-background Trigger now has normalized edge uncertainty 0.2904
(Chrome 0.2774), versus 0.3090 before the redundant FIR was removed. For the 14/16px light/dark
samples, the remaining excess is 2.8–4.7% and total ink differs by at most 1.1%. This remains a
sample-specific diagnostic, not a universal sharpness metric or a pixel-identity claim.

Windows Vulkan also compiled its SPIR-V shaders and passed the real clipped-fade/motion test.
Its optional validation-layer compute test remains unavailable on this host because the
Khronos validation layer is not installed (ERROR_LAYER_NOT_PRESENT); the test requirement is
unchanged. All examples built, and seven non-interactive image-example runs passed.


## Ellipsis soft-break source boundaries

The text shaper is pinned to official pop-os/cosmic-text merge revision
5ea7aae78611652a6c88ba84d8ef3e3099fb32cf. Its merged
[upstream fix #535](https://github.com/pop-os/cosmic-text/pull/535) is not in the
0.19.0 registry release. Both gfx_ui and Tileink use this same source identity;
a consumer does not need a workspace patch or a local fork.

The former release moved a discarded wrap-space onto the last ellipsized line.
That shifted the line's first painted character and consumed room needed by
another character before the ellipsis. The failed-first public text_layout_runs
regression compares Word/WordOrGlyph soft-break source boundaries before/after
limiting the visual line budget. Original Buffer text remains complete. This
updates the shaping root cause; it does not translate glyphs, remove bytes,
change font metrics, or alter Taffy's official implementation.


Validation for this dependency pin: the complete single-thread Release suite, strict
all-target Clippy, formatting, and all example builds passed. Fresh before/after
native rendering of all 1,712 SVG fixtures produced identical decoded pixels under
the same environment. Generated repository images were restored instead of mixing
unrelated historical baseline differences into this dependency change.

## Prepared span paint runs

`TextContext::layout_buffer_runs(font_system, &buffer)` extracts `TextLayoutRun` values from
already-prepared source. It borrows the buffer immutably and performs no shaping. Each result
carries its visual-line index, application metadata, optional straight-alpha sRGB RGBA8 color,
and ordinary TextLayout. Contiguous metadata/color values stay together; different lines and
noncontiguous values stay separate to preserve paint order. Empty paragraphs count toward
visual-line indices while producing no glyph run. The matching font system still owns font ids.

This supplies a native framework seam for per-span paint and line-clamp projection. A uniform
TextLayout intentionally has one caller-supplied brush; flattening styled source into that shape
lost span colors and application identities. Consumers can now change brushes or clip a prepared
run without rebuilding source or reshaping glyphs. Uniform and segmented extraction share one
physical-glyph conversion, retaining UI baselines, marks, cache keys, weight and fractional bins.
Regressions compare every extracted glyph with the existing uniform layout, including mixed
fonts/directions, colors/alpha, metadata, empty paragraphs and a source which must remain unmodified.

Validation for prepared span runs: 925 ordinary single-thread Release library tests passed
(196 existing opt-in tests ignored), plus the integration suite, formatting, strict all-target
Release Clippy and all example builds. The complete 1,712 SVG fixture outputs match the prior
fresh official-tree run pixel-for-pixel with identical adjacent image/font resources. The gfx_ui
consumer's actual mixed-color GPU regression passes immediate/retained recording at 1x, 1.25x
and 2x. Its failed-first uniform path painted red/blue/inherited spans entirely green; prepared
run brushes now retain those declarations without a second source buffer or shaping pass.


### Projecting prepared glyphs

`TextContext::layout_buffer_runs_filtered` keeps the existing shaping and glyph positions while
selecting which complete glyphs reach paint. Its predicate receives the original glyph and visual
line index, including empty-paragraph indices. Empty paint groups are omitted; metadata, color,
baseline and order remain unchanged. The normal `layout_buffer_runs` path includes every glyph.

This is the root seam for block line-clamp projections: pixel clipping alone can cut a ligature,
combining mark or italic outline in half. The application groups clusters before filtering and
retains the full source buffer for selection, copying and later focus/scroll. Neither extraction
method reshapes or mutates that buffer.

## Caller-prepared paragraph layouts

A native wrapping policy can prepare its own `LayoutLine` arrays while retaining the original
Cosmic buffer as the logical text, font and paragraph-direction source. `text_layout_runs_from`
borrows those arrays and applies the same UI baseline, line-height, scroll and visibility rules
as the ordinary buffer view. A callback result of `None` uses the cached paragraph; an empty
slice omits that paragraph without renumbering the remaining logical source indices.

`TextContext::layout_runs_filtered` and `text_hit_runs` consume that same prepared run view.
Their buffer counterparts now delegate to these common paths. This removes the need for a
wrapping policy to rewrite text, clone a second logical buffer, or maintain a parallel painter
and hit tester. It is the layout seam needed for balanced wrapping, not an implementation of
balancing itself. Taffy and Cosmic Text remain official dependencies.

The caller must keep source byte indices, paragraph directions and font-system-local glyph ids
valid. Supplied geometry is immutable during each view. The view neither shapes nor allocates
per glyph. Paint preparation retains the existing metadata/color grouping, glyph-cache keys,
cluster filter and raster/outline conversion; ordinary text adds no new retained layout cache.

Failed-first regressions cover a translated prepared paragraph shared by paint and pointer hits,
source/cache identity, omitted and empty paragraphs, reflow into fractional-height visual rows,
viewport filtering, and byte-for-byte-equivalent default run coordinates and glyph borrowing.
Validation of the prepared-layout seam: 1,047 Release tests across 41 suites passed with
228 existing opt-in tests unchanged. Formatting, strict all-target Release Clippy, all example
builds and seven actual native image-example runs passed. All 1,712 SVG images were byte-identical
to a fresh render of immutable base 1fb6f760, using 3,425 identical adjacent fixture resources.
Generated fixture images were archived for comparison and restored; this change does not
regolden the repository's environment-dependent reference PNGs. Balancing remains a separate
native SDK integration gate. See prepared-text-layout-validation.json for input hashes.

## Borrowed row composition

`text_layout_runs_by(buffer, row)` resolves each absolute paragraph and paragraph-local visual
row through the same line-metric iterator. `None` ends that paragraph; an empty `LayoutLine`
preserves a real blank line. This lets a consumer borrow a short reflowed/clamped prefix and
then return untouched cached source rows, without cloning every glyph in a long hidden tail.
The paragraph-array and default-buffer interfaces delegate to this row resolver. Source text,
font identities, metadata/color order, scrolling, baseline and clipping rules remain unchanged.
A regression checks prefix and tail pointer identity, empty/omitted paragraphs, and shared
paint/hit extraction. This is a source-ownership seam, not another layout or rasterizer.


Validation of borrowed row composition: 1,049 all-target Release tests in 48 suites pass
(228 unchanged opt-in ignores), along with formatting, strict all-target Release Clippy and
all example builds. Six native image examples plus the complete SVG renderer run successfully.
All 1,712 SVG PNGs are byte-identical to the archived fresh prepared-layout baseline; the
3,425 adjacent inputs match its immutable source fixture tree. Generated outputs are archived
and restored, rather than replacing repository goldens. See borrowed-text-rows-validation.json.
