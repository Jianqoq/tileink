# Adjacent-edge circular corner geometry

Native rectangles preserve circular corners using the CSS Backgrounds 3
[overlapping-curves rule](https://www.w3.org/TR/css-backgrounds-3/#corner-overlap).
A shared factor constrains adjacent sums. An individual valid radius can extend
beyond a rectangle midpoint; it must not be clipped independently to half the side.

The root cause was midpoint-quadrant selection followed by independent half-side
clamping. An 8px square with only a 6px top-left corner therefore painted a 4px
curve and disagreed with the source component. Normalizing values alone cannot
fix this because the arc itself crosses the midpoint.

HLSL and Metal now retain the original arithmetic for the ordinary half-side
domain. Larger input uses a common adjacent-edge fit and intersects the rectangle
with applicable circular cuts, preserving the active boundary normal for affine
coverage. The relative normalization uses an exact 2^-64 range change at large
float magnitudes so hardware reciprocal flushing cannot erase finite radii.
This is a geometry/range correction, not an antialiasing threshold or pixel offset.

Fill, stroke, clip and SDF shadows share the same rectangle evaluator. Glass keeps
its existing normal-domain finite-difference arithmetic and routes large corners
through the common distance. Geometry record/encoding layouts remain unchanged.
The early occlusion inset remains a conservative interior proof; it is not the
painted corner clamp. New tests verify every claimed full-tile corner lies inside
the actual CSS silhouette, including non-square boxes and fractional inputs.

The GPU regression compares the actual compiled shader to independent f64
line/arc boundary distances across four corner directions, opposite corners,
overlapping sums, huge finite radii, coordinate scale and affine transforms.
The existing fill/stroke/shadow equivalence matrix also includes cross-midpoint
corners. Negative/NaN validation policy at higher-level UI boundaries is unchanged.

## Validation

Release tests passed: 1052 tests in 41 suites (229 pre-existing opt-in tests ignored).
DX12 and Vulkan each passed all 13 native SDF tests on the explicitly pinned
RTX 4090 with validation enabled. The new line/arc oracle covers 142,956 unique
samples and replays its batch three times on each backend.

All 1,712 full SVG outputs exactly matched the pre-change same-machine baseline.
All examples built; the glass and progressive blur examples also rendered and were
visually inspected. Formatting and strict all-target Release Clippy passed.
[Raw results and input hashes](css-corner-validation/report.json) preserve the test
and shader inputs. These checks establish correctness, not a measured FPS claim.

Metal source is updated in parallel; actual Metal device execution remains an
independent platform acceptance gate.
