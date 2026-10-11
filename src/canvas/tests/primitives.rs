use super::*;
use crate::{
    TextLayoutOptions,
    shared::{
        image::{Image, premul_color_to_rgba8_pack},
        image_resource::{ImageKey, ImageResourceId},
    },
};
use std::rc::Rc;

fn draw_sdf(canvas: &Canvas, index: usize) -> Option<Sdf> {
    canvas.draw_sdf(&canvas.draw_records[index])
}

fn draw_sdf_shadow(canvas: &Canvas, index: usize) -> Option<SdfShadow> {
    canvas.draw_sdf_shadow(&canvas.draw_records[index])
}

fn draw_brush(canvas: &Canvas, index: usize) -> Brush {
    canvas
        .draw_brush_for_record(&canvas.draw_records[index])
        .unwrap()
}

mod append;
mod draws;
mod images;
mod lines;
mod rectangles;
mod shapes;
mod strokes;
mod text;
