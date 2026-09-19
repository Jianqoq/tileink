# M4 native root clear color

`NativeRenderer::set_clear_color` sets the background of subsequent immediate
frames. The default is transparent. It uses the same premultiplied RGBA8 packing
as wgpu, and does not rebuild pipelines or invalidate scene preparation.

Every immediate frame allocates a fresh root surface, including an empty Canvas.
That surface's initial upload contains the packed clear color. Fine rendering loads
this destination, so untouched pixels and partially covered edges preserve the
same background semantics as wgpu without an extra clear dispatch. Changing color
or dimensions cannot reuse old root pixels.

Only the root receives this color. Scratch allocations, local filter contexts and
vector child recorders start transparent. This implements the actual target-clear
contract rather than adding a synthetic background shape to the scene.

Tests compare white, translucent and transparent backgrounds across all four APIs
using empty frames, translucent geometry, resizing, and nested vector images with
opacity filtering. CPU tests inspect root and replacement scratch upload bytes to
check isolation directly. No performance comparison is part of this change.
