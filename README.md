# Tileink

Tileink is a tile-based GPU-compute 2D renderer for Rust. It provides immediate `Canvas` recording and a transactional `RetainedScene`, with paths, text, images, gradients, layers, masks, filters, and SVG.

## Backends

The default feature selects native DX12 on Windows. Select Vulkan on Windows or Linux, or Metal on macOS, with an exclusive feature build:

```sh
cargo test --release -- --test-threads=1
cargo test --release --no-default-features --features vulkan -- --test-threads=1
cargo test --release --no-default-features --features metal -- --test-threads=1
```

Only one GPU backend feature may be enabled in a build. The native renderer owns its context, GPU resources, output textures, and submission receipts. `NativeContextOptions` can select a physical adapter and require API validation.

## Render an image

```rust
use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, NativeBackend, NativeRenderer, Radius};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut canvas = Canvas::new(640, 360, 1.0);
    canvas.push_rect(Rect::new(48.0, 48.0, 280.0, 180.0), Radius::all(24.0), Color::from_rgb8(91, 83, 255));
    let mut renderer = NativeRenderer::new(NativeBackend::Dx12, 640, 360)?;
    renderer.render_to_image(&canvas)?.readback()?.save("out.png")?;
    Ok(())
}
```

Choose `NativeBackend::Vulkan` or `NativeBackend::Metal` with the corresponding Cargo feature and platform. To render directly into a host target, use `NativeRenderer::render_to_target`; see `examples/native_window/`. For long-lived scenes, see [retained scenes](RETAINED_SCENE.md).

## Development

- `cargo fmt --all`
- `cargo clippy --release --all-targets`
- `cargo test --release -- --test-threads=1`
- `scripts/ps1/run_svg_tests.ps1` on Windows, or `scripts/mac/run_svg_tests.sh` on macOS, renders the complete SVG fixture corpus through the native backend.
- `cargo build --release --examples` checks example targets.

The [documentation site](website/docs/intro.md) covers the scene model and rendering pipeline. The [DXC toolchain guide](docs/native/toolchain-discovery.md) describes native shader compilation. [Benchmarks](BENCHMARKS.md) use Criterion.

Licensed under MIT or Apache-2.0.
