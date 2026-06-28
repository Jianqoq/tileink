mod cpu;
mod cubecl;
mod debug;
mod render;
mod scene;
mod shared;
mod svg;

pub const TILE_SIZE: u32 = 16;
pub const TILE_SCALE: f32 = 1.0 / TILE_SIZE as f32;
pub const BLOCK_SIZE: u32 = 16 * 16;

pub use cpu::Renderer as CpuRenderer;
#[cfg(feature = "bench-api")]
pub use cubecl::CubePreparedStage;
pub use cubecl::{Renderer as CubeRenderer, WgpuRenderer as CubeWgpuRenderer};
pub use debug::{
    DebugLineSegment, DebugTileDump, DebugTilePath, DebugTilePathSummary, DebugTileSummary,
    RenderDebugCapture, RenderDebugImage, RenderDebugOptions, RenderDebugText, RenderOptions,
    debug_capture_json,
};
pub use scene::Scene;
pub use shared::{
    bounds::Bounds,
    brush::Brush,
    fill::FillRule,
    image::Image,
    layer::{
        filter::{
            CompositeOperator, ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting, Filter,
            FilterInput, FilterPrimitive, FilterPrimitiveKind, LightSource, MorphologyOperator,
            SpecularLighting,
        },
        region::Region,
    },
    sdf::rect::{Radius, StrokeWidths},
};
pub use svg::{SvgError, SvgOptions};
