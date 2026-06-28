use crate::{
    cpu::computes::filter as filter_compute,
    shared::{
        bounds::Bounds,
        image::Image,
        layer::{
            filter::{self as filter_model, Filter, FilterSurfaceBounds},
            region::Region,
        },
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
        filter_compute::apply(self.image, self.filter, self.bounds);
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
        sample_region: &Region,
        canvas: Bounds,
    ) -> Bounds {
        filter_model::filtered_region_bounds(filter, sample_region, canvas)
    }

    pub fn surface_bounds(
        &self,
        filter: &Filter,
        sample_region: &Region,
        target_bounds: Bounds,
    ) -> Option<FilterSurfaceBounds> {
        filter_model::filter_surface_bounds(filter, sample_region, target_bounds)
    }
}
