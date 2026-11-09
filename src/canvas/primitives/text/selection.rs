//! Foreground changes split only the fine-composition domain, never glyph layout or coverage.
use super::*;

struct Section {
    bounds: PixelBounds,
    highlighted: bool,
}

// One quantization for every shared edge: a pixel belongs to the half-open logical range
// exactly when its center is inside. Outward-rounding both sides would double-composite it.
fn highlight_bounds(rect: Rect, scale: f32) -> Option<PixelBounds> {
    if [rect.x0, rect.y0, rect.x1, rect.y1]
        .iter()
        .any(|v| !v.is_finite())
        || rect.x0 >= rect.x1
        || rect.y0 >= rect.y1
    {
        return None;
    }
    let edge = |value: f64| (value * f64::from(scale) - 0.5).ceil() as i32;
    Some(PixelBounds {
        x0: edge(rect.x0),
        y0: edge(rect.y0),
        x1: edge(rect.x1),
        y1: edge(rect.y1),
    })
}

fn sections(outer: PixelBounds, ranges: &[Rect], scale: f32) -> Vec<Section> {
    let selected = ranges
        .iter()
        .filter_map(|r| highlight_bounds(*r, scale))
        .map(|r| r.intersect(outer))
        .filter(|r| !r.is_empty())
        .collect::<Vec<_>>();
    let mut edges = vec![outer.y0, outer.y1];
    for r in &selected {
        edges.extend([r.y0, r.y1]);
    }
    edges.sort_unstable();
    edges.dedup();
    let mut result: Vec<Section> = Vec::new();
    let mut previous = 0..0;
    let mut intervals = Vec::new();
    let mut row = Vec::new();
    for band in edges.windows(2) {
        let (y0, y1) = (band[0], band[1]);
        intervals.clear();
        row.clear();
        intervals.extend(
            selected
                .iter()
                .filter(|r| r.y0 <= y0 && y0 < r.y1)
                .map(|r| (r.x0, r.x1)),
        );
        intervals.sort_unstable();
        let mut cursor = outer.x0;
        let mut index = 0;
        while index < intervals.len() {
            let start = intervals[index].0;
            let mut end = intervals[index].1;
            index += 1;
            while index < intervals.len() && intervals[index].0 <= end {
                end = end.max(intervals[index].1);
                index += 1;
            }
            if cursor < start {
                row.push(Section {
                    bounds: PixelBounds {
                        x0: cursor,
                        y0,
                        x1: start,
                        y1,
                    },
                    highlighted: false,
                });
            }
            row.push(Section {
                bounds: PixelBounds {
                    x0: start,
                    y0,
                    x1: end,
                    y1,
                },
                highlighted: true,
            });
            cursor = end;
        }
        if cursor < outer.x1 {
            row.push(Section {
                bounds: PixelBounds {
                    x0: cursor,
                    y0,
                    x1: outer.x1,
                    y1,
                },
                highlighted: false,
            });
        }
        let unchanged = previous.len() == row.len()
            && result[previous.clone()].iter().zip(&row).all(|(a, b)| {
                a.highlighted == b.highlighted
                    && a.bounds.x0 == b.bounds.x0
                    && a.bounds.x1 == b.bounds.x1
            });
        if unchanged {
            for section in &mut result[previous.clone()] {
                section.bounds.y1 = y1;
            }
        } else {
            let start = result.len();
            result.append(&mut row);
            previous = start..result.len();
        }
    }
    result
}

impl Canvas {
    /// Adds one prepared glyph run with a different brush inside the union of selection rectangles.
    ///
    /// Glyph keys are recorded once and shared by disjoint brush draws; no shaping or raster work
    /// happens here. Selected coverage is decided at local device-pixel centers before retained
    /// transforms. Adjacent/overlapping selections never blend one pixel twice, including transparent
    /// foregrounds. Invalid/empty selections are ignored. The optional outer clip keeps the ordinary
    /// text overflow contract. Returned IDs address each resulting draw in recording order.
    pub fn push_text_layout_highlighted(
        &mut self,
        layout: &TextLayout,
        origin: Point,
        brush: impl Into<Brush>,
        foreground: impl Into<Brush>,
        ranges: &[Rect],
        clip: Option<Rect>,
    ) -> Vec<DrawId> {
        if layout.is_empty() || !origin.is_finite() {
            return Vec::new();
        }
        let natural = layout_bounds_at_scaled_origin(layout, origin, self.scale_factor());
        let mut outer = PixelBounds {
            x0: natural.x0,
            y0: natural.y0,
            x1: natural.x1,
            y1: natural.y1,
        };
        let clip = match clip {
            Some(rect) => match logical_rect_pixel_bounds(rect, self.scale_factor()) {
                Some(bounds) => {
                    outer = outer.intersect(bounds);
                    Some(bounds)
                }
                None => return Vec::new(),
            },
            None => None,
        };
        if outer.is_empty() {
            return Vec::new();
        }
        let parts = sections(outer, ranges, self.scale_factor());
        let Some(first) = parts.first() else {
            return Vec::new();
        };
        let brush = brush.into();
        let foreground = foreground.into();
        let first_brush = if first.highlighted {
            foreground.clone()
        } else {
            brush.clone()
        };
        let draw_index = self.draw_records.len();
        let Some(id) = self.push_text_layout_inner(layout, origin, clip, first_brush) else {
            return Vec::new();
        };
        let prototype = self.draw_records[draw_index];
        self.draw_records[draw_index].pixel_bounds = first.bounds;
        self.draw_records[draw_index].local_pixel_bounds = first.bounds;
        let mut brushes = [None, None];
        brushes[usize::from(first.highlighted)] =
            Some((prototype.brush_offset, prototype.brush_len));
        let mut ids = Vec::with_capacity(parts.len());
        ids.push(id);
        for part in parts.iter().skip(1) {
            let (brush_offset, brush_len) = *brushes[usize::from(part.highlighted)]
                .get_or_insert_with(|| {
                    self.push_brush(if part.highlighted {
                        foreground.clone()
                    } else {
                        brush.clone()
                    })
                });
            let index = self.push_draw_record(DrawRecord {
                brush_offset,
                brush_len,
                pixel_bounds: part.bounds,
                local_pixel_bounds: part.bounds,
                ..prototype
            });
            self.current_command_list_mut()
                .commands
                .push(Command::Draw(index));
            ids.push(self.draw_id_from_index(index));
        }
        ids
    }
}
