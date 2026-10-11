//! Tables follow the shared execution-plan traversal, including the enclosing
//! filter appended after a localized child plan. Every allocation belongs to the batch.
use super::*;
use crate::native::runtime::program::filter::{brush::Brushes, convolve, transfer, turbulence};
use crate::render::filter_resources::upload::FilterResourceUpload;
use crate::shared::{execution::ExecPlan, layer::filter::Filter};

#[derive(Default)]
pub(super) struct FilterResources {
    pub(super) transfers: Option<transfer::TransferTables>,
    pub(super) convolves: Option<convolve::Kernels>,
    pub(super) turbulence: Option<turbulence::Tables>,
    pub(super) brushes: Option<Brushes>,
}
impl FilterResources {
    pub(super) fn record(
        batch: &mut ComputeBatch,
        plan: &ExecPlan,
        filter: Option<&Filter>,
        images: &Images<'_>,
    ) -> Result<Self> {
        images.validate(batch)?;
        let upload = FilterResourceUpload::new(&plan.ops, filter, Some(images.upload()));
        Ok(Self {
            transfers: if upload.transfers.is_empty() {
                None
            } else {
                Some(transfer::upload(batch, &upload.transfers)?)
            },
            convolves: if upload.convolves.is_empty() {
                None
            } else {
                Some(convolve::upload(batch, &upload.convolves)?)
            },
            turbulence: if upload.turbulence.is_empty() {
                None
            } else {
                Some(turbulence::upload(batch, &upload.turbulence)?)
            },
            brushes: Brushes::record(batch, upload.brushes, images.upload().textures().len())?,
        })
    }
}
