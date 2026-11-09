# Selected text foreground

Canvas::push_text_layout_highlighted records an already prepared TextLayout with an
alternative brush inside the union of caller-owned selection rectangles. It does not
shape or rasterize glyphs. Every brush section references one shared glyph-run record
and cache-key vector; overlapping, adjacent or repeated selection ranges do not paint
the same local device pixel twice. Transparent foreground replaces the ordinary glyph
paint in the range, rather than being overlaid on old ink.

Selection coordinates use half-open rectangles and pixel-center ownership. Shared
edges quantize once with ceil(edge * scale - 0.5). Invalid or empty selection ranges
are ignored. The optional outer text clip retains the ordinary outward-rounded
overflow contract. Brush coordinates remain those of the complete text layout.
Ordinary push_text_layout and push_text_layout_clipped continue to use their compact
single-draw path.

A transformed world AABB is only a coarse rejection bound. Fine glyph composition
also checks the inverse-transformed pixel center against local_pixel_bounds. Without
this check, fractional scale and rotation can overlap two selection sections and
blend old and selected ink into the same pixel. This is a root-cause correction to
draw-local clipping, shared by HLSL (DX12/Vulkan) and Metal; it does not change glyph
masks, font metrics, text transfer rules or draw-record ABI.

Semantic tests cover scales 1/1.25/2/3, fractional origins, overlapping/duplicate ranges,
empty and invalid ranges, transparent foreground, exact ordinary recording, one glyph
run and atomic invalid outer clips. Explicit physical-GPU tests compare the selected
render with independent ordinary and whole-foreground images, pixel by pixel, including
transparent/semitransparent brushes, overflow clipping and retained translation,
fractional scale and rotation. These are glyph-paint tests, not proof of complete
browser/platform selection behavior or universal glyph-raster parity.
