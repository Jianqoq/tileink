//! CPU-only workload for ordinary frames and many deferred child renderers.

use super::UniformWrites;

#[doc(hidden)]
pub struct UniformWriteBenchmark {
    buffers: Vec<[u64; 5]>,
}

impl UniformWriteBenchmark {
    pub fn new(renderers: usize) -> Self {
        assert!(renderers > 0);
        Self {
            buffers: (0..renderers)
                .map(|renderer| std::array::from_fn(|stage| (renderer * 5 + stage) as u64))
                .collect(),
        }
    }

    /// Each renderer records eight batches against five distinct stage buffers.
    /// Include per-frame allocation, aligned padding and final upload traversal.
    pub fn aggregate_frame(&self) -> usize {
        let mut writes = UniformWrites::default();
        let data = [0xa5; 128];
        for buffers in &self.buffers {
            for _ in 0..8 {
                for (&buffer, size) in buffers.iter().zip([64, 32, 64, 64, 128]) {
                    assert!(!writes.is_full(&buffer));
                    writes.write(&buffer, size as u64, 256, 4096, &data[..size]);
                }
            }
        }
        writes
            .iter()
            .map(|(_, bytes)| std::hint::black_box(bytes).len())
            .sum()
    }
}
