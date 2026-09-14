# M4 ordered native texture transfers

ComputeBatch now records one ordered command stream containing dispatch indices
and validated RGBA8 texture copies. Pipeline/descriptor preparation still uses
the dispatch inventory; execution uses the command stream on DX12, Vulkan and
the independent wgpu reference. Copies support subrectangles and array layers,
preserve raw channels without sampling, and reject foreign/non-image/self-copy
resources and checked range overflow. Empty regions record no command.

DX12 transitions source/destination resources to copy states and records each
array subresource. Vulkan transitions GENERAL images to transfer layouts and
restores GENERAL for subsequent shader descriptors. Frame owners retain all GPU
resources, command allocators and submissions through the existing completion
path. Transfers introduce no CPU readback or separate per-copy submission.

Two CPU tests cover domains and command ordering. Two four-API GPU tests compare
exact bytes for copy-only batches, multiple array layers, offset subrectangles,
untouched texels, and compute/copy interleaving (2.53 seconds). Release (1,022),
integrations, formatting, strict Clippy, native-only, full SVG and examples pass;
the 3,471 PNG set contains only the approved turbulence difference. Both review
axes are closed. No performance comparison was run.

This supplies GPU transfer primitives needed by frame history and vector-image
atlas borders. Full frame/vector/effect orchestration and complete four-renderer
acceptance remain; M4 is not complete.