---
title: Progressive blur
---

`ProgressiveBlur` varies blur strength by position: clear before the start, a
smooth transition between the endpoints, and constant maximum blur after the end.

```rust
use peniko::kurbo::{Point, Rect};
use tileink::{Filter, ProgressiveBlur, Radius, Region};

canvas.push_backdrop_layer(
    Filter::ProgressiveBlur(ProgressiveBlur::new(
        Point::new(0.0, 100.0),
        Point::new(0.0, 260.0),
        24.0,
    )),
    Region::rect(Rect::new(0.0, 0.0, 640.0, 360.0), Radius::ZERO),
);
canvas.pop_layer();
```

Paint the background first. Use `push_filter_layer` instead to blur the layer's
own content. Positions and standard deviation use logical pixels and follow the
canvas scale. Swap endpoints to reverse the direction; equal endpoints select
uniform maximum blur. Zero maximum deviation leaves the image unchanged.

The GPU uses a multiscale approximation. This progressively removes detail rather
than fading between the original and one fixed blurred image; it does not promise
exact variable Gaussian convolution. Transparent edges use premultiplied alpha.
Parameters must be finite, sigma must be nonnegative, and device sigma must not
exceed 65536 pixels.

Run `cargo run --release --example progressive_blur` to generate
`target/progressive-blur.png`.
