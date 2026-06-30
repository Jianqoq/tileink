#[cfg(feature = "profile")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MemoryUsage {
    pub(crate) used_bytes: usize,
    pub(crate) allocated_bytes: usize,
}

#[cfg(feature = "profile")]
impl MemoryUsage {
    pub(crate) fn new(used_bytes: usize, allocated_bytes: usize) -> Self {
        Self {
            used_bytes,
            allocated_bytes,
        }
    }

    pub(crate) fn vec<T>(items: &Vec<T>) -> Self {
        let item_size = std::mem::size_of::<T>();
        Self::new(items.len() * item_size, items.capacity() * item_size)
    }

    pub(crate) fn hash_map<K, V, S>(items: &std::collections::HashMap<K, V, S>) -> Self {
        let item_size = std::mem::size_of::<(K, V)>();
        Self::new(items.len() * item_size, items.capacity() * item_size)
    }

    pub(crate) fn sum(items: impl IntoIterator<Item = Self>) -> Self {
        let mut total = Self::default();
        for item in items {
            total.used_bytes += item.used_bytes;
            total.allocated_bytes += item.allocated_bytes;
        }
        total
    }
}
