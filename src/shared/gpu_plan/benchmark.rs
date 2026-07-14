//! Criterion adapter for persistent tile-bin mutation without renderer or GPU timing noise.

use super::*;

#[doc(hidden)]
pub struct TileDrawBinsBenchmark {
    bins: TileDrawBins,
    initial: Vec<DrawRecord>,
    moved: Vec<DrawRecord>,
    draw_order: Vec<u32>,
    changed: Vec<Range<usize>>,
    tiles_size: (u32, u32),
    use_moved: bool,
}

impl TileDrawBinsBenchmark {
    pub fn new(draw_count: usize, changed: Vec<Range<usize>>, spatial_change: bool) -> Self {
        const TILES_SIZE: (u32, u32) = (128, 128);
        let changed_draws = changed
            .iter()
            .flat_map(|range| range.clone())
            .filter(|&draw| draw < draw_count)
            .collect::<HashSet<_>>();
        let mut initial = Canvas::new(
            TILES_SIZE.0 * crate::TILE_SIZE,
            TILES_SIZE.1 * crate::TILE_SIZE,
            1.0,
        );
        let mut moved = Canvas::new(
            TILES_SIZE.0 * crate::TILE_SIZE,
            TILES_SIZE.1 * crate::TILE_SIZE,
            1.0,
        );
        let row_width = TILES_SIZE.0 - 1;
        for draw in 0..draw_count {
            let tile = draw as u32 % (row_width * TILES_SIZE.1);
            let x = tile % row_width;
            let y = tile / row_width;
            let moved_x = x + u32::from(spatial_change && changed_draws.contains(&draw));
            for (canvas, tile_x) in [(&mut initial, x), (&mut moved, moved_x)] {
                let x0 = f64::from(tile_x * crate::TILE_SIZE);
                let y0 = f64::from(y * crate::TILE_SIZE);
                canvas.push_rect(
                    peniko::kurbo::Rect::new(
                        x0,
                        y0,
                        x0 + f64::from(crate::TILE_SIZE),
                        y0 + f64::from(crate::TILE_SIZE),
                    ),
                    crate::Radius::ZERO,
                    peniko::Color::BLACK,
                );
            }
        }
        let plan = initial.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let mut bins = TileDrawBins::default();
        bins.reset(&initial.draw_records, &plan.draw_order, None, TILES_SIZE);
        bins.dirty_records.clear();
        bins.dirty_pages.clear();
        let mut benchmark = Self {
            bins,
            initial: initial.draw_records,
            moved: moved.draw_records,
            draw_order: plan.draw_order.to_vec(),
            changed,
            tiles_size: TILES_SIZE,
            use_moved: true,
        };
        // Move both ways once so page layout and the flat-to-paged transition are outside timing.
        benchmark.update();
        benchmark.update();
        benchmark
    }

    pub fn update(&mut self) -> (usize, usize) {
        let records = if self.use_moved {
            &self.moved
        } else {
            &self.initial
        };
        self.use_moved = !self.use_moved;
        assert!(self.bins.update_changed(
            records,
            &self.draw_order,
            None,
            self.tiles_size,
            &self.changed,
        ));
        let dirty = (self.bins.dirty_records.len(), self.bins.dirty_pages.len());
        self.bins.dirty_records.clear();
        self.bins.dirty_pages.clear();
        self.bins.full_upload = false;
        dirty
    }
}
