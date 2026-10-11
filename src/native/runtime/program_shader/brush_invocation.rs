//! Pixel brush frontend. Filters already store the ordinary public pipeline.
use super::Result;
use crate::native::runtime::compute::{ComputeBatch, ResourceId};
use crate::shared::{filter_config::FilterConfig, shader::ProgramInstance};
pub(crate) struct Output {
    pub(crate) image: ResourceId,
    pub(crate) origin: [u32; 2],
    pub(crate) extent: [u32; 2],
}
pub(crate) fn record(
    batch: &mut ComputeBatch,
    instance: &ProgramInstance,
    region: FilterConfig,
    tiles: Option<&[u32]>,
    input: Option<ResourceId>,
    output: Output,
) -> Result<()> {
    let mut pipeline = instance.program.filter(&instance.uniforms)?;
    pipeline.transform = instance.transform;
    super::pipeline::record_output(batch, &pipeline, region, tiles, input.as_slice(), output)
}
