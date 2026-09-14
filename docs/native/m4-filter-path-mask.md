# M4 exact path masks

The maintained HLSL path-mask entry consumes typed immutable ranges and four
fixed-point coordinate buffers. Rust validates every half-open range, count,
byte address, batch owner and target coordinate domain before dispatch. Empty
paths bind nonempty sentinel storage without reading it. The native interface
uses compact resource slots; the test reference only remaps production WGSL
bindings.

## Root cause and exact predicate

A regression with line `[2147483647, 0, -2147483389, 256]` exposed a shared error:
all four APIs returned transparent at pixel (0,0), while the exact intersection
is 129 fixed-point units, strictly beyond the pixel center at 128. Converting
endpoints to f32 lost the difference and moved the crossing onto the center.

Both shader languages now preserve signed 24.8 coordinates and compare the
orientation determinant without division. Each signed difference is represented
by a sign and unsigned magnitude; two 32-bit words hold each 64-bit product.
Comparing the signed products gives the determinant sign without subtraction
overflow. The implementation uses 16-bit multiplication limbs, needs no GPU
int64 capability, and retains half-open Y intervals and strict X crossing.
This fixes the geometric root cause; no fixture-specific behavior is used.
The coordinate scale is canonical in HLSLI and generated into Rust/WGSL.

## Tests

Independent i128 rational crossing tests cover empty and reversed paths,
self-intersections, winding cancellation and repeated loops; 133 extreme and
seeded coordinate lines test signed differences and multiplication carries.
Additional cases cover half-pixel edges, horizontal/degenerate segments, compact
tiles and nonzero clipped regions. Invalid ranges, foreign resources, indices
and output fixed-point overflow are rejected. All native/portable and texture
table variants run across wgpu/native DX12/Vulkan with exact pixel bytes.


Full release passes 981 ordinary library tests, native runtime passes 115 tests,
and 31 filter GPU tests pass with both DXC executables unavailable and no runtime
compilation. Strict native all-targets release Clippy, Shader Tools roundtrip,
all 67 SPIR-V modules, and shader compiler/inventory/artifact tests pass.
Full SVG/examples preserve all 3,471 PNGs byte-for-byte. Both reviews are closed.
Inventory advances to 135/179; remaining filters, full fine and NativeRenderer/
Canvas integration keep M4 open. Evidence hashes are in the adjacent verification JSON.
