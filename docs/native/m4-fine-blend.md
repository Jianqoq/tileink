# M4 fine blend helpers

Maintained HLSL now implements all sixteen mix and fourteen composition modes.
Mode numbers live in `shared/blend/modes.hlsli`; a host test checks all thirty
against the serialized `peniko::Mix` and `peniko::Compose` values. Separable,
nonseparable and composition math are separate modules without resource globals.
The root blend helper uses vector channels while preserving the operation order.

## Root-cause corrections

An actual four-route test exposed a pre-existing wgpu DX12/Vulkan difference:
Color + DestOver, source `0xfefe7f54`, destination
`0x40000040`, produced blue 181 versus 180. Explicit nested FMA in the luminosity
formula fixes its evaluation order in WGSL and HLSL; the regression independently
requires 181. This is shared arithmetic, not a backend branch or tolerance.

Review also identified missing backdrop endpoint precedence in generic Dodge and
Burn. An independent failing GPU test observed opaque white where Dodge + Copy
requires opaque black. Both languages now preserve a black Dodge backdrop and a
white Burn backdrop before handling source singularities, as specified by
[W3C compositing](https://www.w3.org/TR/compositing-1/#blendingcolordodge).
Copy and SrcIn have independent black/white regression expectations; the existing
SrcOver specializations already used the correct endpoint behavior.

## Verification scope

The four-API corpus contains 428,517 packed-pixel records: every mode combination,
byte alpha boundaries, achromatic/equal-channel/saturated colors, 768 deterministic
random color pairs per combination, four backdrop-endpoint regressions and the
luminosity rounding regression. Clear/Copy/Dest normal blending also has independent
CPU expectations; guard words must survive padded dispatches.

Release verification passed: 966 library tests plus integrations, 64 native
runtime tests, strict all-target Clippy, all header/compiler/inventory checks,
real Shader Tools and 35 SPIR-V modules. All SVG/examples passed; the immutable
3,471 PNG baseline is unchanged. Four math GPU tests passed again with nonexistent
DXC paths and zero new runtime pipeline compilations. Spec and standards reviews
are closed; hashes and local logs are recorded in the verification receipt.

This helper is not a production fine entry:
M4 remains at 27/179 validated inventory entries until fine/effects and the shared
production renderer are integrated.
