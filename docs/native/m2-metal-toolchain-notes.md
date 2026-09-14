# M2 independent Metal shader source and toolchain

Status: planned, not implemented or validated on a Mac. The user clarified on 2026-09-13 that macOS uses its own Metal Shading Language (MSL) source. This replaces the earlier shared-HLSL-to-Metal proposal. M0/M1 runtime scope is unchanged.

DX12 and Vulkan share version-controlled HLSL compiled by DXC to DXIL and SPIR-V. macOS uses independently maintained `.metal` source under `src/shaders/metal/`, compiled by the Apple Metal toolchain. HLSL conversion through DXIL, SPIR-V or another intermediate language is not the macOS source strategy. Existing WGSL remains the independent wgpu reference.

Share the logical program/variant inventory, algorithm and numerical specifications, data ABI, scene/materializer, rendering schedule, input fixtures and exact comparison helpers. Maintain language-specific algorithm implementations and includes separately. A shared semantic fix must update the applicable HLSL, MSL and WGSL implementations and their tests. Backend-specific resource bindings remain explicit target mappings; shared ABI does not assume compiler-default physical layouts are equal.

Keep Metal compilation, artifact validation, reflection/binding diagnostics and tool discovery in a target-specific build module. Cache metadata must include source language, the complete source/include graph, ABI, variants, compiler and SDK identities, language version, target platform/GPU and all optimization options. Wrong-language, stale and incompatible artifacts must be rejected. Ordinary wgpu users do not need Apple shader tools, and DX12/Vulkan builds do not require the Metal toolchain.

M2 establishes MSL clear/copy/layout/sampling probes and verifies entry points, resource bindings, buffer offsets/strides, texture access and numerical behavior. Record the actual Mac GPU, OS, SDK, compiler and flags; compare with same-device wgpu-Metal output byte for byte. Cover rounding, cancellation, degenerate coordinates, sampling edges and the supported non-finite-input contract. Compilation alone cannot complete this gate.

Packaging and negative tests cover missing `.metal`/include files, missing tools, stale caches, ABI mismatches and unsupported capabilities. Minimum platform requirements are selected from measured tool and device support, not inherited from the abandoned shader-converter proposal.

The Windows four-API zero-difference requirement remains unchanged. Cross-OS or cross-GPU global equality is not automatically added. A complete native Metal renderer and its full corpus/performance acceptance remain a later rendering milestone; M2 shader probes do not imply completion of that adapter.
