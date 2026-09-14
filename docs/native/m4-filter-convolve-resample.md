# M4 convolution and resampling

Three maintained HLSL entries add twelve production variants after full acceptance.
They have separate Rust stage modules and explicit resource parameters, sharing
only region recording and small mathematical helpers.

## Convolution

Kernels is a private buffer/length pair. Upload rejects empty, nonfinite and
unsafe accumulation magnitudes; recording checks reversed kernel address range,
target location, modes, finite divisor/bias, signed sample coordinates and ownership.
The algorithm samples straight RGB, applies the reversed kernel and bias, clamps,
then premultiplies using computed or preserved alpha. A zero divisor copies source.

A non-power-of-two negative-wrap regression reproduced a Vulkan error: expected
[91,65,39,255], observed [77,55,33,255]. HLSL mixed-sign remainder is undefined.
The shared integer.hlsli helper now owns the unsigned-magnitude Euclidean remainder
algorithm already used by atlas patterns. Both consumers pass their value and
positive period explicitly. This fixes the cause, with no pixel exception.

Independent integer impulse tests cover both signs, reversal and all edge modes;
a separate f64 oracle covers transparent and partial alpha, two nonzero weights,
nonzero bias/divisor and both preserve-alpha modes. A larger sparse/dense corpus
compares the production variants across all four APIs.

## Resampling

Downsample and upsample share validated source rectangles and logical texture
extents. Downsample bounds cell products and midpoint arithmetic before raw loads.
Upsample uses explicit texel-space bilinear FMA interpolation, independent of
pooled texture capacity. Both retain production fractional endpoint truncation:
empty downsample cells become transparent; an empty upsample source preserves target.

Independent tests cover nearest spatial mapping, box averages, quarter-texel ramps,
two-dimensional bilinear weights, fractional/empty rectangles, 1x1 images, nonzero
source bounds, factors 0/1/2/3/5 and compact partial tiles. Each production native/
portable and texture-table variant is checked across wgpu/native DX12/Vulkan.

## Verification

Focused convolution and resample tests pass (four tests per module). Both
independent reviews are closed. Full release passes 977 ordinary library tests;
native runtime passes 99 tests. Strict native all-targets release Clippy, Shader
Tools roundtrip, 59 SPIR-V modules and shader compiler/inventory/artifact tests pass.
With both DXC executable paths unavailable, 19 filter GPU tests pass without
runtime compilation. Full SVG and examples keep all 3,471 PNGs byte-identical.
Inventory advances to 103/179. M4 remains incomplete: remaining filters, full fine
and NativeRenderer/Canvas still need implementation. Evidence hashes are in the
adjacent m4-filter-convolve-resample-verification.json.
