pub(crate) trait Memory {
    type Buffer<'a>;
    fn allocate(&mut self, size: usize, align: usize) -> Allocation;
    fn capacity(&self) -> usize;
    fn generation(&self) -> u64;
    fn ensure_capacity(&self, min_len: usize);
    fn with_buffer<T>(&self, f: impl FnOnce(Self::Buffer<'_>) -> T) -> T;
    fn write_at(&self, offset: usize, data: &[u8]);
    fn clear(&self);
    fn read_range(&self, offset: usize, size: usize) -> Vec<u8>;
    fn align_up(offset: usize, align: usize) -> usize {
        let align = align.max(1);
        (offset + align - 1) & !(align - 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Allocation {
    pub(crate) offset: usize,
    pub(crate) size: usize,
}
