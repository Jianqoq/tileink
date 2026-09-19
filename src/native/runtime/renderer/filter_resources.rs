//! Tables follow the shared execution-plan traversal, including the enclosing
//! filter appended after a localized child plan. Every allocation belongs to the batch.
use super::*;
use crate::native::runtime::program::filter::{brush::Brushes, convolve, transfer, turbulence};
use crate::render::filter_resources::tables::{
    FilterConvolveUpload, FilterTransferUpload, FilterTurbulenceUpload,
};
use crate::shared::{
    execution::ExecPlan,
    layer::filter::{
        ComponentTransferTable, Filter, TURBULENCE_GRADIENT_LEN, TURBULENCE_TABLE_LEN,
        TurbulenceLattice,
    },
};

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
        let (transfers, convolves, turbulence) = if let Some(filter) = filter {
            (
                FilterTransferUpload::from_ops_and_filter(&plan.ops, filter),
                FilterConvolveUpload::from_ops_and_filter(&plan.ops, filter),
                FilterTurbulenceUpload::from_ops_and_filter(&plan.ops, filter),
            )
        } else {
            (
                FilterTransferUpload::from_plan(plan),
                FilterConvolveUpload::from_plan(plan),
                FilterTurbulenceUpload::from_plan(plan),
            )
        };
        let tables: Vec<ComponentTransferTable> = transfers
            .tables
            .chunks_exact(
                std::mem::size_of::<ComponentTransferTable>() / std::mem::size_of::<u32>(),
            )
            .map(|table| table.try_into().expect("shared transfer table shape"))
            .collect();
        let lattices: Vec<_> = turbulence
            .selectors
            .chunks_exact(TURBULENCE_TABLE_LEN)
            .zip(turbulence.gradients.chunks_exact(TURBULENCE_GRADIENT_LEN))
            .map(|(selectors, gradients)| TurbulenceLattice {
                selectors: selectors.try_into().expect("shared lattice shape"),
                gradients: gradients.to_vec(),
            })
            .collect();
        Ok(Self {
            transfers: if tables.is_empty() {
                None
            } else {
                Some(transfer::upload(batch, &tables)?)
            },
            convolves: if convolves.kernels.is_empty() {
                None
            } else {
                Some(convolve::upload(batch, &convolves.kernels)?)
            },
            turbulence: if lattices.is_empty() {
                None
            } else {
                Some(turbulence::upload(batch, &lattices)?)
            },
            brushes: Brushes::record(batch, &plan.ops, filter, images.upload())?,
        })
    }
}
