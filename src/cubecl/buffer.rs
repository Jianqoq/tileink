use std::marker::PhantomData;

use ::cubecl::{prelude::Runtime, server::Handle};
use bytemuck::Pod;

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

    pub(crate) fn read<R: Runtime>(&self, client: &::cubecl::client::ComputeClient<R>) -> Vec<T> {
        let bytes = client.read_one_unchecked(self.handle.clone());
        bytemuck::cast_slice(&bytes[..bytes_for::<T>(self.len)]).to_vec()
    }
}

fn bytes_for<T>(count: usize) -> usize {
    count * std::mem::size_of::<T>()
}
