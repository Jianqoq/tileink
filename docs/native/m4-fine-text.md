# M4 fine text blending

Maintained HLSL implements integer LCD masks, linear-light alpha/LCD composition,
and perceptual auto-coverage for both mask types. Gamma transfer, basic blending,
apparent-axis correction, coverage curves and automatic composition are separate
modules with explicit direct includes. All functions receive values explicitly;
there are no hidden shader resources or runtime WGSL translation paths.

The 24 perceptual parameters have one canonical HLSLI declaration. The wgpu
build imports their exact floating literals; the production WGSL algorithms and
parameter values are unchanged. This is the fine interpreter's real text math,
not an approximation or fallback.

The probe uses an explicit logical request count. Buffer allocation size alone
must not define a zero-request dispatch: padded storage can otherwise become
visible to a shader. The empty-request regression requires every output sentinel
to remain untouched, and nonempty tests include extra workgroups and a tail guard.

6,018 records exercise transparent, partial/opaque alpha, chromatic backgrounds,
LCD channel masks and coverage edges. Integer LCD values have an independent
channel oracle. Two stable cases additionally use independent f64 sRGB transfer
and premultiplied composition, including black/white coverage 128 yielding 188
in both non-auto linear paths. All five packed outputs match across four APIs;
floating results have no tolerance. Auto-coverage retains the production WGSL
oracle and no-op invariants.

Validation: 992 ordinary release library tests, 145 native runtime tests, final
focused GPU test, strict Clippy, editor/standalone headers, 81 SPIR-V modules,
shader reflection/artifact/cache tests, full SVG tests and examples. The final
GPU test also passes without DXC using persistent pipeline caches. PNG comparison
retains only the previously accepted turbulence baseline change. Both review
axes are closed. No performance comparison was run, per the user's waiver.

This validates fine's text helpers, not the complete fine interpreter or public
NativeRenderer/Canvas integration. M4 production inventory remains 175/179.
