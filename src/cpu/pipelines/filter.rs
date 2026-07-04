use crate::{
    cpu::computes::filter as filter_compute,
    shared::{
        bounds::Bounds,
        image::Image,
        image_resource::ImageResourceStore,
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
    surface_size: (u32, u32),
    backdrop_region: Option<&'a Region>,
    image_resources: Option<&'a ImageResourceStore>,
}

impl<'a> FilterCpuPrepared<'a> {
    pub fn run(&mut self) {
        if let Some(region) = self.backdrop_region {
            filter_compute::apply_backdrop_with_resources(
                self.image,
                self.filter,
                self.bounds,
                self.surface_size,
                region,
                self.image_resources,
            );
        } else {
            filter_compute::apply_with_resources(
                self.image,
                self.filter,
                self.bounds,
                self.image_resources,
            );
        }
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
        image_resources: Option<&'a ImageResourceStore>,
    ) -> FilterCpuPrepared<'a> {
        let surface_size = (image.width, image.height);
        FilterCpuPrepared {
            image,
            filter,
            bounds,
            surface_size,
            backdrop_region: None,
            image_resources,
        }
    }

    pub fn prepare_backdrop<'a>(
        &self,
        image: &'a mut Image,
        filter: &'a Filter,
        bounds: Bounds,
        surface_size: (u32, u32),
        region: &'a Region,
        image_resources: Option<&'a ImageResourceStore>,
    ) -> FilterCpuPrepared<'a> {
        FilterCpuPrepared {
            image,
            filter,
            bounds,
            surface_size,
            backdrop_region: Some(region),
            image_resources,
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
