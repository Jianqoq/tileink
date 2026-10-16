# M4 native texture descriptor tables

Texture tables reference independently sized, owned 2D images without copying
texels or converting them into array layers. The canonical table capacity lives
in `src/shaders/hlsl/shared/texture_table_constants.hlsli`. Tables have no pixel
readback and cannot contain nested tables, foreign handles or array views.

Descriptor counts are explicit in Rust shader interfaces, verified against DXIL
and SPIR-V reflection, and included in the version 3 shader cache identity.
DX12 register ranges must not overlap. Vulkan validates each array element
against per-stage, per-set and aggregate resource limits before layout creation.
Nonuniform sampled-image indexing requires the Vulkan descriptor-indexing feature;
unsupported devices return an error before submission.

Dispatch validation expands table members when checking writable aliases. DX12
state planning likewise expands all members; Vulkan pass barriers cover image
reads and writes. Vulkan descriptor writes live separately from transfer and
command recording in `compute_bindings.rs`. All owned image extents are checked,
including images reachable only through a table.

The four-API semantic test indexes every independently sized image, checks bytes
against a CPU oracle, modifies one table member, and samples again. Host tests
cover ownership, descriptor counts, hidden write aliases and device limits.
Reflection tests reject mismatched counts and confusion between a table and a
single layered texture. The register-overlap regression failed before the fix;
range validation addresses the underlying layout defect.

This is infrastructure for the remaining image-brush kernels. Production shader
inventory remains 167/179; M4 is incomplete until those kernels and the shared
NativeRenderer/Canvas execution path are integrated. Performance comparison is
waived by the user. Final verification is recorded separately.

Validation completed: 992 ordinary release library tests, 141 native runtime
tests, strict Clippy, 77 SPIR-V modules, HLSL editor and standalone-header checks,
full SVG and examples. The four-API texture test replays without DXC and without
pipeline cache misses. The 3,471 PNG set has only the already approved turbulence
baseline difference. Both code-review axes are closed.
