//! Fixed-point path data and range slots shared by filter clipping on every backend.

use crate::shared::{
    execution::{ExecOp, ExecPlan},
    layer::{Layer, region::Region},
    path_flatten::PathFlatten,
};

#[derive(Default)]
pub(crate) struct FilterPathUpload {
    pub(crate) range_starts: Vec<u32>,
    pub(crate) range_ends: Vec<u32>,
    pub(crate) p0x: Vec<i32>,
    pub(crate) p0y: Vec<i32>,
    pub(crate) p1x: Vec<i32>,
    pub(crate) p1y: Vec<i32>,
}

impl FilterPathUpload {
    pub(crate) fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_paths_for_ops(&plan.ops, &mut upload);
        upload
    }

    fn push_region(&mut self, region: &Region) {
        let Region::Path {
            path,
            transform,
            tolerance,
        } = region
        else {
            return;
        };

        let start = self.p0x.len() as u32;
        let path = *transform * path;
        let mut lines = Vec::new();
        PathFlatten::new(&path, *tolerance as f32, self.range_starts.len() as u32)
            .flatten(&mut lines);
        self.p0x.extend(
            lines
                .iter()
                .map(|line| encode_filter_path_coord(line.p0[0])),
        );
        self.p0y.extend(
            lines
                .iter()
                .map(|line| encode_filter_path_coord(line.p0[1])),
        );
        self.p1x.extend(
            lines
                .iter()
                .map(|line| encode_filter_path_coord(line.p1[0])),
        );
        self.p1y.extend(
            lines
                .iter()
                .map(|line| encode_filter_path_coord(line.p1[1])),
        );
        self.range_starts.push(start);
        self.range_ends.push(self.p0x.len() as u32);
    }
}

fn encode_filter_path_coord(value: f32) -> i32 {
    (value * 256.0)
        .round()
        .clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

fn collect_filter_paths_for_ops(ops: &[ExecOp], upload: &mut FilterPathUpload) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                layer, children, ..
            } => match layer {
                Layer::Filter { sample_region, .. } | Layer::Backdrop { sample_region, .. } => {
                    upload.push_region(sample_region);
                    collect_filter_paths_for_ops(children, upload);
                }
                _ => collect_filter_paths_for_ops(children, upload),
            },
            ExecOp::OffscreenMaskLayer {
                layer,
                content,
                mask,
                ..
            } => {
                upload.push_region(&layer.region);
                collect_filter_paths_for_ops(content, upload);
                collect_filter_paths_for_ops(mask, upload);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
