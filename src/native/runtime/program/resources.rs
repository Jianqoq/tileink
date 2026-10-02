use super::super::{
    Result,
    compute::{ComputeBatch, ResourceId},
};

pub(super) fn allocation_size(count: usize, stride: usize) -> Result<usize> {
    count
        .max(1)
        .checked_mul(stride)
        .filter(|&size| size <= u32::MAX as usize)
        .ok_or_else(|| "native scan allocation exceeds raw-buffer address space".into())
}

pub(super) fn allocate(
    batch: &mut ComputeBatch,
    count: usize,
    stride: usize,
) -> Result<ResourceId> {
    let size = allocation_size(count, stride)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size)?;
    bytes.resize(size, 0);
    batch.buffer(bytes)
}

pub(super) fn upload<T: bytemuck::Pod>(
    batch: &mut ComputeBatch,
    records: &[T],
) -> Result<ResourceId> {
    if records.is_empty() {
        allocate(batch, 1, size_of::<T>())
    } else {
        let size = allocation_size(records.len(), size_of::<T>())?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(size)?;
        bytes.extend_from_slice(bytemuck::cast_slice(records));
        batch.buffer(bytes)
    }
}
