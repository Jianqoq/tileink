//! Pixel brush frontend. Filters already store the ordinary public pipeline.
use super::Result;
use crate::native::runtime::compute::{ComputeBatch, ResourceId};
use crate::shared::{filter_config::FilterConfig, shader::BrushInstance};
pub(crate) struct Output {
    pub(crate) image: ResourceId,
    pub(crate) origin: [u32; 2],
    pub(crate) extent: [u32; 2],
}
pub(crate) fn record(
    batch: &mut ComputeBatch,
    instance: &BrushInstance,
    region: FilterConfig,
    output: Output,
) -> Result<()> {
    let mut pipeline = instance.program.filter(&instance.uniforms)?;
    pipeline.transform = instance.transform;
    // Procedural brushes fill their draw rectangle without filter inputs or tile lists.
    super::pipeline::record_output(batch, &pipeline, region, None, &[], output)
}
