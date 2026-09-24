# Native output contract

A native build selects exactly one of `dx12`, `vulkan`, or `metal`. `NativeContext` owns the selected device and queue. `NativeRenderer` records a `Canvas` or `RetainedScene` and submits it to a renderer-owned image, a native texture, or an imported host target.

An image output is a single-layer, single-mip, 2D RGBA8 UNORM image. `NativeImageSubmission::readback` waits for completion and yields premultiplied RGBA8 pixels. Target imports require matching device identity, size, format, and usage. The host controls presentation and must honor the returned submission receipt before reusing resources.

An explicit backend or physical adapter request never silently selects another device. API validation requests fail when validation is unavailable.
