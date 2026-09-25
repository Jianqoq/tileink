//! Renderer-side tile debug capture shared by renderers, examples, and tests.
//!
//! The renderer owns collection because it has direct access to scan buffers and
//! final pixels. Callers own persistence: `RenderDebugCapture` contains named
//! text/image artifacts and an output folder, but it never writes files itself.

use crate::shared::{fill::FillRule, image::Image};
use peniko::Color;
use std::{
    fmt::Write,
    io,
    path::{Path, PathBuf},
};

/// Optional render-time controls that keep normal rendering free of debug work.
#[derive(Clone, Debug, Default)]
pub struct RenderOptions {
    pub debug: Option<RenderDebugOptions>,
}

/// Tile debug configuration for one render call.
///
/// Debug output requires an explicit output folder so callers can save the
/// capture without inventing paths later. The renderer only records the path in
/// the capture; the caller decides when to create the folder and write files.
#[derive(Clone, Debug)]
pub struct RenderDebugOptions {
    output_dir: PathBuf,
    tile: Option<(u32, u32)>,
    tile_overlay: Option<TileOverlayOptions>,
}

impl RenderDebugOptions {
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self::try_new(output_dir).expect("debug output_dir must be a folder path")
    }

    pub fn try_new(output_dir: impl Into<PathBuf>) -> io::Result<Self> {
        let output_dir = output_dir.into();
        if output_dir.as_os_str().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "debug output_dir must be a folder path",
            ));
        }
        if output_dir.exists() && !output_dir.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("debug output_dir is not a folder: {}", output_dir.display()),
            ));
        }
        Ok(Self {
            output_dir,
            tile: None,
            tile_overlay: None,
        })
    }

    pub fn with_tile(mut self, tile: (u32, u32)) -> Self {
        self.tile = Some(tile);
        self
    }

    /// Adds a pixel-aligned tile grid image named `final_tiles.png` to the debug capture.
    pub fn with_tile_overlay(mut self, grid_color: Color, text_color: Color) -> Self {
        self.tile_overlay = Some(TileOverlayOptions {
            grid_color,
            text_color,
        });
        self
    }

    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    pub fn tile(&self) -> Option<(u32, u32)> {
        self.tile
    }

    pub fn tile_overlay(&self) -> Option<TileOverlayOptions> {
        self.tile_overlay
    }
}

/// Pixel overlay settings for render debug captures.
///
/// The renderer writes this as a separate final-image copy so normal render
/// output remains unchanged while tile coordinates stay directly comparable to
/// the captured pixels.
#[derive(Clone, Copy, Debug)]
pub struct TileOverlayOptions {
    pub grid_color: Color,
    pub text_color: Color,
}

/// Backend-neutral data captured by renderers without performing file IO.
#[derive(Clone, Debug, Default)]
pub struct RenderDebugCapture {
    pub backend: String,
    pub output_dir: PathBuf,
    pub texts: Vec<RenderDebugText>,
    pub images: Vec<RenderDebugImage>,
    pub tiles: Vec<DebugTileSummary>,
    pub tile: Option<DebugTileDump>,
}

#[derive(Clone, Debug)]
pub struct RenderDebugText {
    pub name: String,
    pub contents: String,
}

#[derive(Clone, Debug)]
pub struct RenderDebugImage {
    pub name: String,
    pub image: Image,
}

#[derive(Clone, Debug)]
pub struct DebugTileSummary {
    pub tile_x: u32,
    pub tile_y: u32,
    pub paths: Vec<DebugTilePathSummary>,
}

#[derive(Clone, Debug)]
pub struct DebugTilePathSummary {
    pub draw_ix: Option<usize>,
    pub path_id: u32,
    pub fill_rule: FillRule,
    pub backdrop: i32,
    pub segment_start: u32,
    pub segment_end: u32,
    pub segment_count: u32,
}

#[derive(Clone, Debug)]
pub struct DebugTileDump {
    pub tile_x: u32,
    pub tile_y: u32,
    pub alpha: Vec<u8>,
    pub final_rgba: Vec<[u8; 4]>,
    pub paths: Vec<DebugTilePath>,
}

#[derive(Clone, Debug)]
pub struct DebugTilePath {
    pub draw_ix: Option<usize>,
    pub path_id: u32,
    pub fill_rule: FillRule,
    pub backdrop: i32,
    pub segment_start: u32,
    pub segment_end: u32,
    pub alpha: Vec<u8>,
    pub segments: Vec<DebugLineSegment>,
}

#[derive(Clone, Copy, Debug)]
pub struct DebugLineSegment {
    pub point0: [f32; 2],
    pub point1: [f32; 2],
    pub y_edge: f32,
}

pub fn debug_capture_json(capture: &RenderDebugCapture) -> String {
    let specific = capture.tile.as_ref().map_or_else(
        || "null".to_string(),
        |tile| format!("{{\"tile_x\":{},\"tile_y\":{}}}", tile.tile_x, tile.tile_y),
    );
    let text_names = json_string_array(capture.texts.iter().map(|text| text.name.as_str()));
    let image_names = json_string_array(capture.images.iter().map(|image| image.name.as_str()));
    format!(
        concat!(
            "{{\n",
            "  \"backend\": \"{}\",\n",
            "  \"output_dir\": \"{}\",\n",
            "  \"tile_count\": {},\n",
            "  \"specific_tile\": {},\n",
            "  \"texts\": {},\n",
            "  \"images\": {}\n",
            "}}\n"
        ),
        escape_json(&capture.backend),
        escape_json(&capture.output_dir.display().to_string()),
        capture.tiles.len(),
        specific,
        text_names,
        image_names
    )
}

fn json_string_array<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::from("[");
    for (ix, item) in items.enumerate() {
        if ix > 0 {
            out.push(',');
        }
        let _ = write!(out, "\"{}\"", escape_json(item));
    }
    out.push(']');
    out
}

fn escape_json(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
