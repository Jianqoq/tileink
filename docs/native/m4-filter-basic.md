# M4 basic filter kernels

Six maintained HLSL production entries implement clear, copy, source alpha, tile,
offset and drop-shadow mask. Each is compared byte-for-byte with actual production
WGSL in all four native/web and texture-table on/off variants, on wgpu DX12/Vulkan
and native DX12/Vulkan. These account for 24 inventory entries after verification;
other filters, full fine and NativeRenderer/Canvas integration remain outstanding.

`FilterConfig` is a shared plain Rust host type. HLSLI declares its parameters
explicitly; reflection checks actual Rust field offsets and U32/I32/F32 scalar or
four-lane vector types. The versioned interface cache key includes scalar types.
No ABI JSON or runtime translation is involved. The prior uint-only reflection
could not describe signed offsets or float filter parameters; this extends the
real contract, without representing float fields as unsigned bit aliases.

The safe encoder validates matching independent textures, checked region bounds,
signed offset arithmetic, finite bounded source rectangles, unique in-range compact
tiles and padded dispatch addressing. Live counts derive from region/list lengths.
An empty region/list records no dispatch. `dispatch_width == 0` selects the maximum
legal row width; a positive value caps it for the caller's scheduling constraints.

Independent CPU expected pixels cover dense and reverse sparse tiles, odd extents,
clipped regions, untouched output, zero/nonzero alpha, signed offsets, zero-width
source tiles, and nonzero fractional tile rectangle endpoints. Small dispatch row
caps exercise padded 2D launches. Every output pixel is compared on all routes.

The existing real editor regression reproduced HLSL0052 when passing a constant
buffer into an ordinary struct parameter. Helpers now explicitly accept
`ConstantBuffer<FilterConfig>`, eliminating the implicit conversion at its source;
all HLSL/editor round trips pass. DXC also checks every standalone helper header.

Full release (971 ordinary tests), all 75 runtime tests, strict Clippy, 46 embedded
SPIR-V modules, shader header/editor/artifact checks and both reviews pass. No-DXC
replay compiles zero pipelines. Full SVG/examples preserve all 3,471 PNG hashes.
M4 remains incomplete at 51/179 production inventory entries; these are kernel
checks, not native full-frame renderer acceptance.

The frame-filter integration additionally accepts tile cells crossing the local
surface boundary. HLSL now explicitly saturates negative floating endpoints before
unsigned conversion, matching WGSL, and returns transparent texels outside the
logical source. The host bounds positive endpoints and wrap arithmetic rather
than requiring the entire cell inside the texture. This fixes rejection/undefined
conversion after localization; it is not a fixture exception. A focused CPU test
first reproduced the rejection; four-route tests cover negative, oversized, empty
and wholly external cells against an independent integer pixel oracle.
