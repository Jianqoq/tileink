use crate::{
    cpu::computes::filter,
    shared::{
        bounds::Bounds,
        image::Image,
        layer::{filter::Filter, region::Region},
    },
};

pub struct FilterCpuPipeline;

pub struct FilterCpuPrepared<'a> {
    image: &'a mut Image,
    filter: &'a Filter,
    bounds: Bounds,
}

impl<'a> FilterCpuPrepared<'a> {
    pub fn run(&mut self) {
        filter::apply(self.image, self.filter, self.bounds);
    }
}

impl FilterCpuPipeline {
    pub fn new() -> Self {
        Self
    }

    pub fn prepare<'a>(
        &self,
        image: &'a mut Image,
        filter: &'a Filter,
        bounds: Bounds,
    ) -> FilterCpuPrepared<'a> {
        FilterCpuPrepared {
            image,
            filter,
            bounds,
        }
    }

    pub fn filtered_region_bounds(
        &self,
        filter: &Filter,
        region: &Region,
        canvas: Bounds,
    ) -> Bounds {
        filter::filtered_region_bounds(filter, region, canvas)
    }

    pub fn rasterize_region_mask(&self, region: &Region, bounds: Bounds) -> Image {
        filter::rasterize_region_mask(region, bounds)
    }
}
