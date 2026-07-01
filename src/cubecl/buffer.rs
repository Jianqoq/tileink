use std::marker::PhantomData;

use ::cubecl::{prelude::Runtime, server::Handle};
use bytemuck::Pod;

#[cfg(feature = "profile")]
use crate::shared::memory::MemoryUsage;

/// Typed owner for a CubeCL contiguous buffer handle.
///
/// CubeCL owns the actual storage and reuses allocations through its runtime
/// memory manager. The renderer still records element lengths explicitly so
/// kernels never depend on inferred byte sizes or dynamic append behavior.
pub(crate) struct CubeBuffer<T> {
    handle: Handle,
    len: usize,
    capacity_bytes: usize,
    marker: PhantomData<T>,
}

impl<T: Pod> CubeBuffer<T> {
    pub(crate) fn new<R: Runtime>(
        client: &::cubecl::client::ComputeClient<R>,
        capacity: usize,
    ) -> Self {
        let capacity_bytes = bytes_for::<T>(capacity).max(1);
        Self {
            handle: client.empty(capacity_bytes),
            len: 0,
            capacity_bytes,
            marker: PhantomData,
        }
    }

    pub(crate) fn replace<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        data: &[T],
    ) {
        let bytes = bytemuck::cast_slice(data);
        self.handle = if bytes.is_empty() {
            client.empty(1)
        } else {
            client.create_from_slice(bytes)
        };
        self.len = data.len();
        self.capacity_bytes = bytes.len().max(1);
    }

    /// Replaces the logical contents while growing allocation capacity by powers of two.
    ///
    /// CubeCL 0.10 exposes whole-buffer uploads through `create_from_slice`, not a safe
    /// subrange write API. Padding the upload to the grown capacity keeps the backing
    /// allocation stable in size for atlas-like data, so repeated renders do not bounce
    /// between exact allocation sizes. Callers should use this for data that changes
    /// rarely, such as glyph atlas metadata and pixels.
    pub(crate) fn replace_growing<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        data: &[T],
    ) {
        self.len = data.len();
        if data.is_empty() {
            return;
        }

        let capacity = grow_capacity(self.capacity(), data.len());
        let capacity_bytes = bytes_for::<T>(capacity).max(1);
        let data_bytes = bytemuck::cast_slice(data);
        self.handle = if data_bytes.len() == capacity_bytes {
            client.create_from_slice(data_bytes)
        } else {
            let mut bytes = vec![0; capacity_bytes];
            bytes[..data_bytes.len()].copy_from_slice(data_bytes);
            client.create_from_slice(&bytes)
        };
        self.capacity_bytes = capacity_bytes;
    }

    pub(crate) fn reserve<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        min_capacity: usize,
    ) {
        let bytes = bytes_for::<T>(min_capacity).max(1);
        if bytes > self.capacity_bytes {
            self.handle = client.empty(bytes);
            self.capacity_bytes = bytes;
        }
        self.len = self.len.min(min_capacity);
    }

    pub(crate) fn resize_uninit<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        len: usize,
    ) {
        self.reserve(client, len);
        self.len = len;
    }

    pub(crate) unsafe fn arg<R: Runtime>(&self) -> ::cubecl::prelude::ArrayArg<R> {
        unsafe { ::cubecl::prelude::ArrayArg::from_raw_parts(self.handle.clone(), self.len) }
    }

    #[cfg(feature = "wgpu")]
    pub(crate) fn handle(&self) -> Handle {
        self.handle.clone()
    }

    pub(crate) fn read<R: Runtime>(&self, client: &::cubecl::client::ComputeClient<R>) -> Vec<T> {
        let bytes = client.read_one_unchecked(self.handle.clone());
        bytemuck::cast_slice(&bytes[..bytes_for::<T>(self.len)]).to_vec()
    }

    #[cfg(feature = "profile")]
    pub(crate) fn memory_usage(&self) -> MemoryUsage {
        MemoryUsage::new(bytes_for::<T>(self.len), self.capacity_bytes)
    }

    fn capacity(&self) -> usize {
        self.capacity_bytes / std::mem::size_of::<T>()
    }
}

fn grow_capacity(current: usize, required: usize) -> usize {
    if required <= current {
        return current;
    }
    let mut capacity = current.max(1);
    while capacity < required {
        capacity = capacity.saturating_mul(2);
        assert!(capacity != 0, "CubeBuffer capacity overflow");
    }
    capacity
}

fn bytes_for<T>(count: usize) -> usize {
    count * std::mem::size_of::<T>()
}

#[cfg(test)]
mod tests {
    use super::grow_capacity;

    #[test]
    fn grow_capacity_doubles_until_required() {
        assert_eq!(grow_capacity(0, 0), 0);
        assert_eq!(grow_capacity(0, 1), 1);
        assert_eq!(grow_capacity(1, 2), 2);
        assert_eq!(grow_capacity(2, 3), 4);
        assert_eq!(grow_capacity(4, 5), 8);
        assert_eq!(grow_capacity(8, 8), 8);
    }
}
