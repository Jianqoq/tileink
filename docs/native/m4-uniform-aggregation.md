# M4 native uniform aggregation

DX12 and Vulkan use one shared pass-usage analysis to pack immutable uniform
buffers. A resource qualifies only when it is used as a uniform, has no storage
read/write use anywhere in the batch, and is not an explicit readback output.
Classification covers the entire batch before recording native commands; a later
alias must not turn mutable data into an immutable upload snapshot.

Each slot retains its original bytes, with zero padding and the backend's required
alignment. Sizes and allocation failures are checked. Native descriptors point to
their own offsets; dispatches cannot overwrite earlier constants. Shader constants
and layouts remain defined by the existing HLSLI/reflection contract.

DX12 retains a shared upload-heap allocation in GENERIC_READ and binds aligned CBV
addresses into it. Packed constants never receive resource transitions. Other
resources preserve existing copies, UAV barriers and readbacks. Resource allocation
and ownership now live in `dx12/compute_resources.rs`, separately from command
ordering in `dx12/compute.rs`.

Vulkan packs constants into the frame's existing coherent upload arena and omits
their device-local buffer/copy allocations. Descriptor offsets respect the device
uniform alignment. Host writes and allocation ownership follow the same existing
submission path as internal dispatch-grid uniforms. Both APIs retain all backing
allocations through their submission owner until completion.

This implements aggregation at the actual native resource boundary, rather than
merely batching CPU parameter writes. No shader behavior, hidden readback or extra
submission is introduced. Performance comparisons remain waived by the user.

Validation covers aligned distinct slots, padding, empty/unused resources, invalid
alignment, storage aliases and readback exclusions. The four-API GPU case uses three
different dispatch regions and explicitly reads back the first uniform, verifying
both packed slots and independent readback semantics against exact CPU bytes.
The focused four-API test passed (0.86 s). Complete frame filters, groups/masks and
text all passed exact four-API comparisons across the reference variants (three
tests, 489.05 s). Release validation passed 1,039 ordinary unit tests plus
integrations, formatting, strict Clippy, native-only builds, full SVG and examples.
All 3,471 PNGs were compared with the preserved baseline; only the approved
turbulence change remains. Both reviews closed with no outstanding findings.

M4 is still incomplete: shared full-frame/public renderer assembly and complete
immediate SVG/example four-renderer acceptance remain.
