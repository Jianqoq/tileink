# Foreground Gaussian groups

`Canvas::push_blur_group(sigma)` and `RetainedLayerDescriptor::BlurGroup` blur a complete
subtree. This is the primitive used by native Motion foreground blur; backdrop blur remains
a separate effect. Unlike an explicit SVG filter region, neither a layout rectangle nor a
fragment canvas extent clips the group. The compiler derives its physical input region from
accepted descendant paint, including off-canvas input and nested effects, and the existing
Gaussian filter expands the output by its kernel footprint.

The logical sigma is validated and converted once at recording. Retained damage expands through
this boundary using the encoded physical sigma. Empty subtrees stay empty. A parameter or child
geometry revision recompiles the dependent filter plan; clean siblings retain their existing
paint identities. This fixes the missing native capability itself, without approximating a
foreground blur using a backdrop or root-rectangle clip.

Release tests cover DPI conversion, empty/nested groups, overflow, incremental child growth,
replacement/removal and sigma changes against fresh retained and immediate GPU output.
