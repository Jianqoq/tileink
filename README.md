# Tileink

[![CI](https://github.com/Jianqoq/tileink/actions/workflows/ci.yml/badge.svg)](https://github.com/Jianqoq/tileink/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/tileink.svg)](https://crates.io/crates/tileink)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Tileink is a tile-based, GPU-compute 2D renderer for Rust and WGPU. It combines an immediate
`Canvas` with a transactional, incremental `RetainedScene` for interfaces and other large scenes
where only a small part changes from frame to frame.

Tileink supports paths, analytic SDF primitives, text, images, gradients, layers, masks, filters,
backdrops, and SVG. Both scene models converge on the same coarse-to-fine tile pipeline and can
render to renderer-owned output, transient WGPU textures, or persistent WGPU textures with
explicit output-history identity.

Release notes are maintained in the [changelog](CHANGELOG.md).

The [native HLSL backend plan](NATIVE_BACKEND_PLAN.md) describes planned, opt-in DX12/Vulkan
backends and exact pixel parity with wgpu. [M0 reference validation](NATIVE_BACKEND_PROGRESS.md)
is in progress; native API backends are not implemented yet.

> [!IMPORTANT]
> Tileink is an early-stage project. The public API, rendering behavior, and performance profile
> are still evolving; evaluate it against your own scenes before adopting it in production.

## Quick start

This creates a small immediate scene, renders it on the default WGPU device, and saves the result
as `out.png`:

```rust
use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, Radius, WgpuRenderer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut canvas = Canvas::new(640, 360, 1.0);
    canvas.push_rect(
        Rect::new(48.0, 48.0, 280.0, 180.0),
        Radius::all(24.0),
        Color::from_rgb8(91, 83, 255),
    );

    let mut renderer = WgpuRenderer::new_default_device(640, 360, Color::TRANSPARENT);
    renderer.render(&canvas);
    renderer.image().save("out.png")?;
    Ok(())
}
```

Canvas coordinates are logical pixels; the scale factor controls physical output size. Use
`Canvas` for one-shot or fully rebuilt scenes and [`RetainedScene`](RETAINED_SCENE.md) when a large
scene receives mostly local updates.

### Present to a WGPU surface

Interactive applications can construct `Renderer` from the same device and queue used by their
surface, render directly into the acquired surface texture, and then present it:

```rust
use peniko::Color;
use tileink::{Canvas, Renderer};

fn create_renderer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
) -> Renderer {
    Renderer::new(device, queue, width, height, Color::WHITE)
}

fn draw(
    renderer: &mut Renderer,
    scene: &Canvas,
    surface: &wgpu::Surface<'_>,
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> Result<(), Box<dyn std::error::Error>> {
    let frame = match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
        wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
            return Ok(());
        }
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            surface.configure(device, config);
            return Ok(());
        }
        wgpu::CurrentSurfaceTexture::Validation => {
            return Err(std::io::Error::other("surface validation error").into());
        }
    };

    renderer.render_to_wgpu_texture(scene, &frame.texture)?;
    renderer.queue().present(frame);
    Ok(())
}
```

Direct surface rendering requires an `Rgba8Unorm` surface format. The complete example requests
`RENDER_ATTACHMENT | STORAGE_BINDING | COPY_SRC | COPY_DST` so the same target works with Tileink's
native and portable WGPU paths. If the surface cannot expose those usages, render to a compatible
intermediate texture and copy or blit it to the acquired surface texture. The complete
[`winit_svg_tiger` example](https://github.com/Jianqoq/tileink/blob/main/examples/winit_svg_tiger.rs)
includes device creation, capability validation, resize handling, surface recovery, rendering, and
presentation:

```powershell
cargo run --release --example winit_svg_tiger
```

### Retained updates

`RetainedScene` gives stable identities to independently changing parts of a scene. Insert content
once, then commit small structural, content, or transform changes without rebuilding unrelated
nodes:

```rust
let root = RetainedNodeId::for_owner(1);
let card = RetainedNodeId::for_owner(2);
let mut scene = RetainedScene::new(640, 360, 1.0, root)?;

scene.transaction()
    .insert_scene(
        RetainedParent::content(root),
        None,
        card,
        Rc::new(card_canvas),
        Affine::IDENTITY,
    )
    .commit()?;
renderer.render_retained(&scene);

scene.transaction()
    .set_transform(card, Affine::translate((120.0, 40.0)))
    .commit()?;
renderer.render_retained(&scene);
```

The complete [`retained` example](examples/retained.rs) writes before-and-after PNGs into `target`:

```powershell
cargo run --release --example retained
```

## SVG support

SVG is a first-class Tileink input rather than a separate raster backend. `Canvas::push_svg` and
`Canvas::push_svg_with_options` lower a normalized [`usvg`](https://github.com/linebender/resvg/tree/main/crates/usvg)
tree into the same path, image, text, layer, mask, and filter pipeline used by manually recorded
content. Lowering is transactional: unsupported or invalid semantics return `SvgError` without
partially changing the destination canvas.

The current static-SVG path covers shapes and arbitrary paths, fill and stroke styling, linear and
radial gradients, patterns, markers, nested transforms and view boxes, raster images, text converted
to paths, clipping, masking, opacity and blend groups, and SVG filter primitives. Dynamic SVG
features such as scripting, events, and animation are outside the scope of `usvg` and Tileink.

Tileink validates this path with the public [resvg](https://github.com/linebender/resvg) SVG corpus:

- More than 1,700 fixtures cover filters, masking, paint servers, painting, shapes, structure, and
  text.
- The committed reference PNGs are rendered by resvg; Tileink's WGPU PNGs are generated beside
  them for regression and visual review.
- Native and portable WGPU output is compared pixel-for-pixel. Small, explainable edge-coverage or
  text-rasterization differences from the resvg reference are reviewed rather than hidden behind a
  broad tolerance.

Run the complete release-mode SVG matrix with:

```powershell
.\scripts\ps1\run_svg_tests.ps1
```

On macOS, use `./scripts/mac/run_svg_tests.sh`. See the
[SVG guide](website/i18n/en/docusaurus-plugin-content-docs/current/guides/svg.md) for API options.

## Platform and backend status

The levels below describe Tileink's current integration and test confidence, not every platform
that WGPU can theoretically target:

| Level | Meaning |
|---|---|
| **Primary** | Regular development path with Tileink-specific optimizations and release validation |
| **Supported** | First-party runner or well-defined integration path; less coverage than the primary path |
| **Provisional** | Expected to work through WGPU, but not covered by Tileink's current full release matrix |
| **Not claimed** | Upstream WGPU capability exists, but Tileink does not yet publish a supported integration |

### Operating systems

| System | Level | Graphics path | Notes |
|---|---|---|---|
| Windows 10/11 | **Primary** | DX12; Vulkan or GL through WGPU | Main development and benchmark path; compatible DX12 devices can use build-time DXIL |
| macOS | **Supported** | Metal through WGPU | First-party release scripts cover tests, examples, and the SVG matrix |
| Linux desktop | **Provisional** | Vulkan or GL/GLES through WGPU | Portable core, but no published Tileink full-matrix runner or CI result yet |
| Web/WASM | **Not claimed** | Browser WebGPU or WebGL2 upstream | No Tileink browser example or browser test matrix yet |
| iOS/Android | **Not claimed** | Metal, Vulkan, or GLES upstream | No first-party surface integration or mobile validation matrix yet |

### Rendering backends and paths

| Backend or path | Level | Tileink behavior |
|---|---|---|
| WGPU native fine path | **Primary** | Uses storage-texture capabilities when the adapter exposes the required features |
| WGPU portable fine path | **Primary** | Uses intermediate textures and explicit copies; examples and SVG fixtures are pixel-compared with the native path |
| Direct3D 12 | **Primary** | WGSL works through WGPU; compatible Windows builds can embed DXIL for the primary portable-fine shaders, with automatic WGSL fallback |
| Metal | **Supported** | Standard WGPU shader path, exercised by the macOS release runners |
| Vulkan | **Provisional** | Standard WGPU shader path; available on supported native WGPU systems but not part of the current published Tileink matrix |
| OpenGL/GLES | **Provisional** | Portable WGPU path; WGPU itself classifies GL as a secondary backend |
| Browser WebGPU/WebGL2 | **Not claimed** | Upstream WGPU backend exists, but Tileink does not yet ship or validate a browser integration |

WGPU's own backend definitions and platform availability are documented in
[`wgpu::Backends`](https://docs.rs/wgpu/30.0.0/wgpu/struct.Backends.html). Applications can provide
their own `wgpu::Device` and `wgpu::Queue`; `WgpuRenderer::new_default_device` is only a convenience
for native applications.

## Why Tileink?

Tileink is designed around workloads that repeatedly update a small region of a large UI scene:

- **Persistent local updates.** Stable node identity, a bounded change journal, damage tracking,
  reusable scene arenas, tile membership, batch state, and output history let retained rendering
  reuse unchanged work.
- **Transactional scene mutation.** Insert, replace, move, reparent, resize, and invalidation
  operations are validated and committed atomically. A failed transaction cannot expose a partial
  hierarchy.
- **UI-oriented analytic primitives.** Rectangles, rounded rectangles, circles, lines, arcs,
  shadows, checkerboards, and candlesticks have analytic SDF paths alongside general vector paths.
- **Integrated effects and content.** Text shaping/rasterization, image resources, SVG lowering,
  masks, filters, and backdrop effects use the same renderer and retained damage model.
- **Observable incremental behavior.** Profiling APIs expose stage timings, uploaded bytes, damage,
  tile rewrites, plan fragments, and related counters. `ForceFull` remains available as a
  correctness and performance oracle.
- **DX12 startup path.** Compatible Windows builds embed build-time DXIL for the primary portable
  fine shaders, with automatic WGSL fallback when the backend, device, or layout does not match.

Windows builders can set `TILEINK_DXIL_CACHE_DIR` to a persistent directory to reuse generated
DXIL across fresh Cargo `OUT_DIR`s and CI workspaces. Entries are content-addressed by the expanded
shader, generator implementation, locked Naga version, and DXC toolchain files; restored binaries
are checksum-verified before they are embedded. Set `TILEINK_DXIL_PRECOMPILE=0` to disable the
build-time DXIL path entirely.

These are architectural advantages for Tileink's target workload, not a claim that Tileink is
universally faster than another renderer. Scene structure, dirty ratio, effects, resolution, GPU,
driver, and output path all materially affect results.

## Inspired by Vello

Tileink is heavily inspired by [Vello](https://github.com/linebender/vello). Vello demonstrated how
a Rust 2D renderer can move traditionally sequential vector work onto GPU compute using scans and
coarse-to-fine parallelism. Tileink shares that compute-first direction, uses the same Rust graphics
ecosystem around WGPU and Peniko/Kurbo, and owes a clear conceptual debt to the Vello project and
the research it builds on.

Tileink is not intended to be a drop-in Vello replacement. Its public scene model and engineering
focus diverge around transactional retained state, explicit damage/output history, specialized
analytic UI primitives, and an integrated text/SVG/effects stack. Vello remains the more established
project, with a larger community, broader ecosystem, and substantially more real-world exposure.

## Credits and acknowledgements

- **GPU vector rendering:** Tileink's overall compute-first direction is heavily inspired by the
  [Vello](https://github.com/linebender/vello) project and its underlying research.
- **Liquid Glass:** Tileink's Liquid Glass effect implementation draws implementation and visual
  inspiration from [iyinchao/liquid-glass-studio](https://github.com/iyinchao/liquid-glass-studio),
  an [MIT-licensed](https://github.com/iyinchao/liquid-glass-studio/blob/main/LICENSE) WebGL2/WebGPU
  exploration of SDF-shaped glass, refraction, dispersion, Fresnel reflection, glare, and
  multipass blur.

## Tileink compared with Vello

This table compares the current Tileink repository with
[Vello 0.9.0](https://docs.rs/vello/0.9.0/vello/), the optional comparison dependency pinned by this
repository. It describes public APIs and design focus rather than promising benchmark results.

| Dimension | Tileink | Vello 0.9.0 |
|---|---|---|
| Primary design center | Retained desktop/UI scenes with frequent local changes, plus an immediate path | General-purpose, high-throughput 2D vector rendering through GPU compute |
| Public scene model | Immediate `Canvas` and transactional `RetainedScene` with stable node IDs | [`Scene`](https://docs.rs/vello/0.9.0/vello/struct.Scene.html), an encoded drawing-command sequence that can be reused unchanged or reset and rebuilt |
| Incremental updates | Explicit transactions, bounded journal, persistent materialization, damage propagation, tile/batch reuse, and output history | No public retained transaction/journal equivalent; applications submit a `Scene`, while renderer-owned caches and resources remain internal |
| Raster pipeline | Path scan/prefix work, coarse 16×16 tile binning, fine compute raster, and analytic SDF evaluation | Compute-centric vector pipeline using prefix-sum algorithms; configurable area AA, 8× MSAA, or 16× MSAA |
| Specialized UI geometry | First-class analytic SDF rectangles, circles, lines, arcs, shadows, checkerboards, and candlesticks | General path, fill, stroke, image, layer, and glyph APIs; specialized UI forms are typically expressed through those primitives |
| Effects | Integrated opacity, blend, masks, filters, backdrop filters, and retained offscreen-plan identity | Layers and blending are available; Vello's official README lists blur and filter effects as ongoing work |
| Text | Integrated Cosmic Text shaping, Swash rasterization, prepared glyph/run reconciliation, and retained glyph-image reuse | Glyph drawing is supported; the official project status lists glyph caching as ongoing work |
| SVG | `usvg` lowering is integrated into `Canvas` and uses the same text/image/filter pipeline | SVG integration is provided separately by [`vello_svg`](https://github.com/linebender/vello_svg) |
| WGPU output | Convenience or application-owned device; renderer-owned, transient texture, and persistent texture-history APIs | Application-owned WGPU context and rendering to a compatible texture, commonly followed by a surface blit |
| Validation and diagnostics | Atomic retained validation, explicit incremental stats, profiled render entry points, and `ForceFull` comparison mode | Renderer errors, debug facilities, and a mature set of project test scenes; no matching public retained-state validator is required by its scene model |
| Maturity and ecosystem | Young `0.1` project with an evolving API and small ecosystem | Established Linebender project; still officially alpha, but used by Xilem and supported by a broader ecosystem |
| Consider it when | Local-update cost, retained hierarchy, integrated UI effects, or explicit damage control are central | A more established general vector renderer, existing integrations, or Vello's scene/API model is the better fit |

## Performance comparisons

The repository contains opt-in, same-workload comparison programs against its pinned Vello version:

```powershell
cargo run --release --features vello-compare --example vello_compare
cargo run --release --features vello-compare --example svg_tiger_vello_compare
```

Treat their output as local measurements, not universal rankings. A useful comparison must use the
same logical scene, output size, warm-up policy, synchronization boundary, adapter class, and as
close to the same WGPU path as dependency versions allow. Retained dirty-scene benchmarks and their
methodology are documented in [`BENCHMARKS.md`](BENCHMARKS.md).

## Documentation

The TypeScript/React documentation site lives in [`website`](website). The default command builds
and serves both Chinese and English, so the language selector works locally:

```powershell
cd website
npm install
npm run start
```

For faster single-language development with hot reload, use `npm run dev` for Chinese or
`npm run dev:en` for English. Docusaurus development mode serves only one locale at a time, so its
language selector cannot switch to the other locale; use `npm run start` when testing localization.

Production validation:

```powershell
npm run typecheck
npm run build
```

Rust API rustdoc can be generated with `cargo doc --no-deps --open`. See
[`RETAINED_SCENE.md`](RETAINED_SCENE.md) for the retained model and the English architecture guide
under [`website/i18n/en/docusaurus-plugin-content-docs/current/architecture`](website/i18n/en/docusaurus-plugin-content-docs/current/architecture)
for the pipeline and damage model.

## Contributing and security

Contributions are welcome. Read [`CONTRIBUTING.md`](CONTRIBUTING.md) before opening a pull request.
Please report security vulnerabilities privately as described in [`SECURITY.md`](SECURITY.md), not
in a public issue.

## License

Tileink is licensed under either the [Apache License, Version 2.0](LICENSE-APACHE) or the
[MIT license](LICENSE-MIT), at your option.

## Development

```powershell
cargo test --release -- --test-threads=1
```

See [`AGENTS.md`](AGENTS.md) for the repository's complete test and review policy.
