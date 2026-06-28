//! Renderer-side tile debug capture shared by CPU, CubeCL, examples, and tests.
//!
//! The renderer owns collection because it has direct access to scan buffers and
//! final pixels. Callers own persistence: `RenderDebugCapture` contains named
//! text/image artifacts and an output folder, but it never writes files itself.

use std::{
    fmt::Write,
    io,
    path::{Path, PathBuf},
};

use crate::{
    TILE_SIZE,
    cpu::computes::fine::build_tile_alpha,
    scene::Scene,
    shared::{
        fill::FillRule,
        image::{Image, rgba8_pack},
        line_seg::LineSegment,
        tile_seg_range::TileSegmentRange,
    },
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
        })
    }

    pub fn with_tile(mut self, tile: (u32, u32)) -> Self {
        self.tile = Some(tile);
        self
    }

    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    pub fn tile(&self) -> Option<(u32, u32)> {
        self.tile
    }
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

pub(crate) struct DebugScanBuffers<'a> {
    pub(crate) backdrops: &'a [i32],
    pub(crate) tile_segment_ranges: &'a [TileSegmentRange],
    pub(crate) segments: &'a [LineSegment],
}

pub(crate) fn capture_render_debug(
    backend: &str,
    scene: &Scene,
    final_image: &Image,
    scan: DebugScanBuffers<'_>,
    options: &RenderOptions,
) -> RenderDebugCapture {
    let Some(debug) = &options.debug else {
        return RenderDebugCapture {
            backend: backend.to_string(),
            ..RenderDebugCapture::default()
        };
    };

    let tiles = capture_all_tiles(scene, &scan);
    let mut capture = RenderDebugCapture {
        backend: backend.to_string(),
        output_dir: debug.output_dir.clone(),
        texts: vec![
            RenderDebugText {
                name: "tiles.json".to_string(),
                contents: tiles_json(scene, &tiles),
            },
            RenderDebugText {
                name: "tiles.svg".to_string(),
                contents: tiles_svg(scene, &tiles),
            },
        ],
        images: Vec::new(),
        tiles,
        tile: None,
    };

    if let Some((tile_x, tile_y)) = debug
        .tile()
        .filter(|&(x, y)| x < scene.width_in_tiles() && y < scene.height_in_tiles())
    {
        let tile = capture_tile_detail(scene, final_image, &scan, tile_x, tile_y);
        capture.texts.push(RenderDebugText {
            name: "tile.json".to_string(),
            contents: tile_json(&tile),
        });
        capture.texts.push(RenderDebugText {
            name: "tile.svg".to_string(),
            contents: tile_svg(&tile),
        });
        capture.images.push(RenderDebugImage {
            name: "tile_alpha.png".to_string(),
            image: alpha_image(&tile.alpha),
        });
        capture.images.push(RenderDebugImage {
            name: "tile_final.png".to_string(),
            image: tile_final_image(final_image, tile_x, tile_y),
        });
        capture.tile = Some(tile);
    }

    let capture_json = debug_capture_json(&capture);
    capture.texts.push(RenderDebugText {
        name: "capture.json".to_string(),
        contents: capture_json,
    });
    capture
}

fn capture_all_tiles(scene: &Scene, scan: &DebugScanBuffers<'_>) -> Vec<DebugTileSummary> {
    let mut tiles = Vec::with_capacity((scene.width_in_tiles() * scene.height_in_tiles()) as usize);
    for tile_y in 0..scene.height_in_tiles() {
        for tile_x in 0..scene.width_in_tiles() {
            tiles.push(DebugTileSummary {
                tile_x,
                tile_y,
                paths: capture_tile_path_summaries(scene, scan, tile_x, tile_y),
            });
        }
    }
    tiles
}

fn capture_tile_detail(
    scene: &Scene,
    final_image: &Image,
    scan: &DebugScanBuffers<'_>,
    tile_x: u32,
    tile_y: u32,
) -> DebugTileDump {
    let mut tile_alpha = vec![0; (TILE_SIZE * TILE_SIZE) as usize];
    let paths = capture_tile_path_summaries(scene, scan, tile_x, tile_y)
        .into_iter()
        .map(|summary| {
            let segment_slice = scan
                .segments
                .get(summary.segment_start as usize..summary.segment_end as usize)
                .unwrap_or_default();
            let alpha = build_tile_alpha(segment_slice, summary.backdrop, summary.fill_rule);
            combine_debug_alpha(&mut tile_alpha, &alpha);
            let segments = segment_slice
                .iter()
                .map(|segment| DebugLineSegment {
                    point0: [segment.point0.0, segment.point0.1],
                    point1: [segment.point1.0, segment.point1.1],
                    y_edge: segment.y_edge,
                })
                .collect();
            DebugTilePath {
                draw_ix: summary.draw_ix,
                path_id: summary.path_id,
                fill_rule: summary.fill_rule,
                backdrop: summary.backdrop,
                segment_start: summary.segment_start,
                segment_end: summary.segment_end,
                alpha: alpha.to_vec(),
                segments,
            }
        })
        .collect();

    DebugTileDump {
        tile_x,
        tile_y,
        alpha: tile_alpha,
        final_rgba: tile_final_rgba(final_image, tile_x, tile_y),
        paths,
    }
}

fn capture_tile_path_summaries(
    scene: &Scene,
    scan: &DebugScanBuffers<'_>,
    tile_x: u32,
    tile_y: u32,
) -> Vec<DebugTilePathSummary> {
    let mut paths = Vec::new();
    for record in &scene.bd_records {
        if tile_x < record.tile_x0
            || tile_x >= record.tile_x1
            || tile_y < record.tile_y0
            || tile_y >= record.tile_y1
        {
            continue;
        }
        let stride = record.tile_x1 - record.tile_x0;
        if stride == 0 {
            continue;
        }
        let local_x = tile_x - record.tile_x0;
        let local_y = tile_y - record.tile_y0;
        let backdrop_ix = (record.data_offset + local_y * stride + local_x) as usize;
        let Some(&range) = scan.tile_segment_ranges.get(backdrop_ix) else {
            continue;
        };
        let Some(&backdrop) = scan.backdrops.get(backdrop_ix) else {
            continue;
        };
        if backdrop == 0 && range.start == range.end {
            continue;
        }

        let draw_ix = scene
            .draw_records
            .iter()
            .position(|draw| draw.path_id == Some(record.path_id));
        let fill_rule = draw_ix
            .and_then(|ix| scene.draw_records.get(ix))
            .map(|draw| draw.fill_rule)
            .unwrap_or(FillRule::NonZero);
        paths.push(DebugTilePathSummary {
            draw_ix,
            path_id: record.path_id,
            fill_rule,
            backdrop,
            segment_start: range.start,
            segment_end: range.end,
            segment_count: range.end.saturating_sub(range.start),
        });
    }
    paths
}

fn combine_debug_alpha(target: &mut [u8], alpha: &[u8; 256]) {
    for (dst, &src) in target.iter_mut().zip(alpha) {
        *dst = (*dst).max(src);
    }
}

fn alpha_image(alpha: &[u8]) -> Image {
    let mut pixels = Vec::with_capacity((TILE_SIZE * TILE_SIZE) as usize);
    for &a in alpha {
        pixels.push(rgba8_pack([a, a, a, 255]));
    }
    Image {
        width: TILE_SIZE,
        height: TILE_SIZE,
        pixels,
    }
}

fn tile_final_image(final_image: &Image, tile_x: u32, tile_y: u32) -> Image {
    Image {
        width: TILE_SIZE,
        height: TILE_SIZE,
        pixels: tile_final_rgba(final_image, tile_x, tile_y)
            .into_iter()
            .map(rgba8_pack)
            .collect(),
    }
}

fn tile_final_rgba(final_image: &Image, tile_x: u32, tile_y: u32) -> Vec<[u8; 4]> {
    let mut rgba = Vec::with_capacity((TILE_SIZE * TILE_SIZE) as usize);
    let x0 = tile_x * TILE_SIZE;
    let y0 = tile_y * TILE_SIZE;
    for local_y in 0..TILE_SIZE {
        for local_x in 0..TILE_SIZE {
            let x = x0 + local_x;
            let y = y0 + local_y;
            rgba.push(if x < final_image.width && y < final_image.height {
                final_image.rgba8_at(x, y)
            } else {
                [0, 0, 0, 0]
            });
        }
    }
    rgba
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

fn tiles_json(scene: &Scene, tiles: &[DebugTileSummary]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"width_in_tiles\": {},", scene.width_in_tiles());
    let _ = writeln!(out, "  \"height_in_tiles\": {},", scene.height_in_tiles());
    let _ = writeln!(out, "  \"tiles\": [");
    for (tile_ix, tile) in tiles.iter().enumerate() {
        let comma = if tile_ix + 1 == tiles.len() { "" } else { "," };
        let _ = writeln!(
            out,
            "    {{\"tile_x\":{},\"tile_y\":{},\"paths\":[",
            tile.tile_x, tile.tile_y
        );
        write_path_summaries_json(&mut out, &tile.paths, "      ");
        let _ = writeln!(out, "    ]}}{}", comma);
    }
    out.push_str("  ]\n}\n");
    out
}

fn tile_json(tile: &DebugTileDump) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"tile_x\": {},", tile.tile_x);
    let _ = writeln!(out, "  \"tile_y\": {},", tile.tile_y);
    out.push_str("  \"alpha\": ");
    write_alpha_rows_json(&mut out, &tile.alpha);
    out.push_str(",\n  \"final_rgba\": ");
    write_rgba_rows_json(&mut out, &tile.final_rgba);
    out.push_str(",\n  \"paths\": [\n");
    for (path_ix, path) in tile.paths.iter().enumerate() {
        let comma = if path_ix + 1 == tile.paths.len() {
            ""
        } else {
            ","
        };
        let _ = write!(
            out,
            concat!(
                "    {{\"draw_ix\":{},\"path_id\":{},\"fill_rule\":\"{:?}\",",
                "\"backdrop\":{},\"tile_segment_range\":[{},{}],\"alpha\":"
            ),
            json_option_usize(path.draw_ix),
            path.path_id,
            path.fill_rule,
            path.backdrop,
            path.segment_start,
            path.segment_end
        );
        write_alpha_rows_json(&mut out, &path.alpha);
        out.push_str(",\"clipped_segments\":[");
        for (seg_ix, segment) in path.segments.iter().enumerate() {
            let seg_comma = if seg_ix + 1 == path.segments.len() {
                ""
            } else {
                ","
            };
            let _ = write!(
                out,
                "{{\"p0\":[{},{}],\"p1\":[{},{}],\"y_edge\":{}}}{}",
                segment.point0[0],
                segment.point0[1],
                segment.point1[0],
                segment.point1[1],
                segment.y_edge,
                seg_comma
            );
        }
        let _ = writeln!(out, "]}}{}", comma);
    }
    out.push_str("  ]\n}\n");
    out
}

fn write_path_summaries_json(out: &mut String, paths: &[DebugTilePathSummary], indent: &str) {
    for (path_ix, path) in paths.iter().enumerate() {
        let comma = if path_ix + 1 == paths.len() { "" } else { "," };
        let _ = writeln!(
            out,
            concat!(
                "{}{{\"draw_ix\":{},\"path_id\":{},\"fill_rule\":\"{:?}\",",
                "\"backdrop\":{},\"tile_segment_range\":[{},{}],\"segment_count\":{}}}{}"
            ),
            indent,
            json_option_usize(path.draw_ix),
            path.path_id,
            path.fill_rule,
            path.backdrop,
            path.segment_start,
            path.segment_end,
            path.segment_count,
            comma
        );
    }
}

fn write_alpha_rows_json(out: &mut String, alpha: &[u8]) {
    out.push('[');
    for y in 0..TILE_SIZE {
        if y > 0 {
            out.push(',');
        }
        out.push('[');
        for x in 0..TILE_SIZE {
            if x > 0 {
                out.push(',');
            }
            let ix = (y * TILE_SIZE + x) as usize;
            let _ = write!(out, "{}", alpha.get(ix).copied().unwrap_or(0));
        }
        out.push(']');
    }
    out.push(']');
}

fn write_rgba_rows_json(out: &mut String, rgba: &[[u8; 4]]) {
    out.push('[');
    for y in 0..TILE_SIZE {
        if y > 0 {
            out.push(',');
        }
        out.push('[');
        for x in 0..TILE_SIZE {
            if x > 0 {
                out.push(',');
            }
            let px = rgba
                .get((y * TILE_SIZE + x) as usize)
                .copied()
                .unwrap_or([0, 0, 0, 0]);
            let _ = write!(out, "[{},{},{},{}]", px[0], px[1], px[2], px[3]);
        }
        out.push(']');
    }
    out.push(']');
}

fn tiles_svg(scene: &Scene, tiles: &[DebugTileSummary]) -> String {
    let tile_px = 28;
    let width = scene.width_in_tiles() * tile_px + 1;
    let height = scene.height_in_tiles() * tile_px + 1;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\">",
        width, height, width, height
    );
    out.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>\n");
    for tile in tiles {
        let x = tile.tile_x * tile_px;
        let y = tile.tile_y * tile_px;
        let active = !tile.paths.is_empty();
        let max_backdrop = tile
            .paths
            .iter()
            .map(|path| path.backdrop.unsigned_abs())
            .max()
            .unwrap_or(0);
        let fill = if active { "#dbeafe" } else { "#ffffff" };
        let stroke = if max_backdrop > 0 {
            "#dc2626"
        } else {
            "#94a3b8"
        };
        let _ = writeln!(
            out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" stroke=\"{}\"/>",
            x, y, tile_px, tile_px, fill, stroke
        );
        let _ = writeln!(
            out,
            "<text x=\"{}\" y=\"{}\" font-size=\"7\" font-family=\"monospace\" fill=\"#0f172a\">{},{} </text>",
            x + 3,
            y + 9,
            tile.tile_x,
            tile.tile_y
        );
        if active {
            let _ = writeln!(
                out,
                "<text x=\"{}\" y=\"{}\" font-size=\"7\" font-family=\"monospace\" fill=\"#1d4ed8\">scan:{}</text>",
                x + 3,
                y + 20,
                tile.paths.len()
            );
            out.push_str("<title>");
            let _ = write!(out, "tile {},{}; ", tile.tile_x, tile.tile_y);
            for path in &tile.paths {
                let _ = write!(
                    out,
                    "scan path {} backdrop {} range {}..{} segments {}; ",
                    path.path_id,
                    path.backdrop,
                    path.segment_start,
                    path.segment_end,
                    path.segment_count
                );
            }
            out.push_str("</title>\n");
        }
    }
    out.push_str("</svg>\n");
    out
}

fn tile_svg(tile: &DebugTileDump) -> String {
    let cell = 24.0;
    let grid = TILE_SIZE as f32 * cell;
    let width = grid + 300.0;
    let height = grid.max(80.0 + tile.paths.len() as f32 * 48.0);
    let colors = [
        "#dc2626", "#2563eb", "#16a34a", "#9333ea", "#ea580c", "#0891b2", "#be123c",
    ];
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\">",
        width, height, width, height
    );
    out.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>\n");
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let alpha = tile.alpha[(y * TILE_SIZE + x) as usize];
            let _ = writeln!(
                out,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"rgb({},{},{})\"/>",
                x as f32 * cell,
                y as f32 * cell,
                cell,
                cell,
                alpha,
                alpha,
                alpha
            );
        }
    }
    for i in 0..=TILE_SIZE {
        let p = i as f32 * cell;
        let _ = writeln!(
            out,
            "<line x1=\"{}\" y1=\"0\" x2=\"{}\" y2=\"{}\" stroke=\"#cbd5e1\" stroke-width=\"0.5\"/>",
            p, p, grid
        );
        let _ = writeln!(
            out,
            "<line x1=\"0\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#cbd5e1\" stroke-width=\"0.5\"/>",
            p, grid, p
        );
    }
    for (path_ix, path) in tile.paths.iter().enumerate() {
        let color = colors[path_ix % colors.len()];
        for segment in &path.segments {
            let _ = writeln!(
                out,
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"2\" stroke-linecap=\"round\"/>",
                segment.point0[0] * cell,
                segment.point0[1] * cell,
                segment.point1[0] * cell,
                segment.point1[1] * cell,
                color
            );
        }
    }
    let side_x = grid + 20.0;
    let _ = writeln!(
        out,
        "<text x=\"{}\" y=\"24\" font-size=\"16\" font-family=\"monospace\" fill=\"#0f172a\">tile {},{}</text>",
        side_x, tile.tile_x, tile.tile_y
    );
    for (path_ix, path) in tile.paths.iter().enumerate() {
        let color = colors[path_ix % colors.len()];
        let y = 56.0 + path_ix as f32 * 48.0;
        let _ = writeln!(
            out,
            "<rect x=\"{}\" y=\"{}\" width=\"12\" height=\"12\" fill=\"{}\"/>",
            side_x,
            y - 10.0,
            color
        );
        let _ = writeln!(
            out,
            "<text x=\"{}\" y=\"{}\" font-size=\"12\" font-family=\"monospace\" fill=\"#0f172a\">path {} draw {:?}</text>",
            side_x + 18.0,
            y,
            path.path_id,
            path.draw_ix
        );
        let _ = writeln!(
            out,
            "<text x=\"{}\" y=\"{}\" font-size=\"12\" font-family=\"monospace\" fill=\"#475569\">backdrop {} range {}..{} segments {}</text>",
            side_x + 18.0,
            y + 16.0,
            path.backdrop,
            path.segment_start,
            path.segment_end,
            path.segments.len()
        );
    }
    out.push_str("</svg>\n");
    out
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

fn json_option_usize(value: Option<usize>) -> String {
    value.map_or_else(|| "null".to_string(), |value| value.to_string())
}

fn escape_json(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use peniko::{
        Color,
        kurbo::{Affine, Rect, Shape},
    };

    use super::{RenderDebugOptions, RenderOptions};
    use crate::{FillRule, Scene, cpu::Renderer};

    #[test]
    fn cpu_render_with_options_captures_tile_debug_outputs() {
        let mut scene = Scene::new(32, 32);
        scene.push_path(
            Rect::new(4.0, 4.0, 20.0, 20.0).to_path(0.1),
            Color::from_rgb8(0, 128, 0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.1,
        );

        let output_dir = PathBuf::from("target/debug-capture-test");
        let options = RenderOptions {
            debug: Some(RenderDebugOptions::new(&output_dir).with_tile((0, 0))),
        };
        let mut renderer = Renderer::new(32, 32, Color::TRANSPARENT);

        let capture = renderer.render_with_options(&scene, &options);

        assert_eq!(capture.backend, "cpu");
        assert_eq!(capture.output_dir, output_dir);
        assert_eq!(capture.tiles.len(), 4);
        assert!(capture.texts.iter().any(|text| text.name == "capture.json"));
        assert!(capture.texts.iter().any(|text| text.name == "tiles.json"));
        assert!(capture.texts.iter().any(|text| text.name == "tiles.svg"));
        assert!(capture.texts.iter().any(|text| text.name == "tile.json"));
        assert!(capture.texts.iter().any(|text| text.name == "tile.svg"));
        let tiles_svg = capture
            .texts
            .iter()
            .find(|text| text.name == "tiles.svg")
            .expect("overview svg");
        assert!(tiles_svg.contents.contains("scan:"));
        assert!(!tiles_svg.contents.contains(">path:"));
        assert!(
            capture
                .images
                .iter()
                .any(|image| image.name == "tile_alpha.png")
        );
        assert!(
            capture
                .images
                .iter()
                .any(|image| image.name == "tile_final.png")
        );
        let tile = capture.tile.as_ref().expect("specific tile dump");
        assert_eq!((tile.tile_x, tile.tile_y), (0, 0));
        assert_eq!(tile.alpha.len(), 256);
        assert_eq!(tile.final_rgba.len(), 256);
        assert!(tile.alpha.iter().any(|alpha| *alpha > 0));
        assert!(!tile.paths.is_empty());
    }

    #[test]
    fn debug_options_reject_existing_file_output_path() {
        let dir = PathBuf::from("target/debug-capture-test");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("not-a-folder");
        fs::write(&file, b"not a folder").unwrap();

        assert!(RenderDebugOptions::try_new(file).is_err());
    }
}
