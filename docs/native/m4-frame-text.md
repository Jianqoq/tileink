# M4 native frame text preparation

Native frame execution accepts the same prepared glyph/run data as the shared
scene uploader. Preparation uses the caller's FontSystem and TextContext: font
IDs belong to that font database and must not be resolved using a newly created
unrelated database.

Root and localized filter scenes share the prepared data. Shared localization
transforms draw placement while retaining root glyph/run indices; rebuilding a
local atlas would break that association. Every scene still owns its batch GPU
uploads, so a nested filter cannot overwrite a parent's text resources.

Without prepared text, glyph draws remain disabled, matching the existing wgpu
render entry point. No implicit font loading or fallback renderer is added.

The complete-frame regression uses a checked-in font, nonintegral glyph origins,
non-tile-aligned clipping and text inside a localized filter. It compares both
production wgpu Renderers and native DX12/Vulkan with exact RGBA bytes across all
fine/filter variants. Before wiring preparation into frame execution, the test
reproduced background pixels where glyph coverage was expected.

Validation passed: exact four-API pixels for None/RGB/BGR coverage, sRGB/linear
compositing, chunked/nonchunked coarse, root/localized text and disabling text on
a reused scene cache (156.42 s). A CPU fixture assertion confirms the checked-in
Noto CBDT and Source Sans fonts generate actual Color and SubpixelMask images.
Release unit/integration tests, strict Clippy, native-only checks, full SVG and
examples passed. The 3,471 PNG baseline still has only the approved turbulence
difference. Both reviews closed without outstanding findings. No performance
comparison was run.

This step does not complete M4: public NativeRenderer
assembly, pooled uniforms/submission integration and full immediate SVG/example
four-renderer acceptance remain.
