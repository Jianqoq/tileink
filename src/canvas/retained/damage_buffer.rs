use crate::shared::bounds::Bounds;

/// One traversal buffer, with a separate visible range for each offscreen input.
/// This removes per-layer work-vector allocations without allowing damage from
/// the parent or a sibling texture to enter an isolated input.
pub(super) struct DamageBuffer {
    values: Vec<Bounds>,
    start: usize,
}

pub(super) struct DamageScope {
    parent_start: usize,
    parent_end: usize,
}

impl DamageBuffer {
    pub(super) fn new(seeds: &[Bounds]) -> Self {
        Self {
            values: seeds.to_vec(),
            start: 0,
        }
    }

    pub(super) fn current(&self) -> &[Bounds] {
        &self.values[self.start..]
    }

    pub(super) fn push_unique(&mut self, bounds: Bounds) {
        if !bounds.is_empty() && !self.current().contains(&bounds) {
            self.values.push(bounds);
        }
    }

    pub(super) fn enter_scope(&mut self, seeds: &[Bounds]) -> DamageScope {
        let scope = DamageScope {
            parent_start: self.start,
            parent_end: self.values.len(),
        };
        self.start = scope.parent_end;
        self.values.extend_from_slice(seeds);
        scope
    }

    pub(super) fn finish_scope(&mut self, scope: DamageScope) {
        debug_assert_eq!(self.start, scope.parent_end);
        let mut write = scope.parent_end;
        for read in scope.parent_end..self.values.len() {
            let bounds = self.values[read];
            if !bounds.is_empty() && !self.values[scope.parent_start..write].contains(&bounds) {
                // Compaction only writes at or behind the current read position.
                self.values[write] = bounds;
                write += 1;
            }
        }
        self.values.truncate(write);
        self.start = scope.parent_start;
    }

    pub(super) fn into_vec(self) -> Vec<Bounds> {
        debug_assert_eq!(self.start, 0);
        self.values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: i32) -> Bounds {
        Bounds::new(x, 0, x + 1, 1)
    }

    #[test]
    fn isolated_inputs_hide_parent_and_sibling_damage() {
        let mut buffer = DamageBuffer::new(&[bounds(1)]);
        let content = buffer.enter_scope(&[bounds(-4)]);
        assert_eq!(buffer.current(), &[bounds(-4)]);
        buffer.push_unique(bounds(2));
        buffer.finish_scope(content);
        let mask = buffer.enter_scope(&[bounds(-4)]);
        assert_eq!(buffer.current(), &[bounds(-4)]);
        buffer.push_unique(bounds(3));
        buffer.finish_scope(mask);
        assert_eq!(
            buffer.into_vec(),
            vec![bounds(1), bounds(-4), bounds(2), bounds(3)]
        );
    }

    #[test]
    fn nested_scopes_merge_unique_damage_in_painter_order() {
        let mut buffer = DamageBuffer::new(&[bounds(1)]);
        let outer = buffer.enter_scope(&[]);
        buffer.push_unique(bounds(2));
        let inner = buffer.enter_scope(&[bounds(2), bounds(2)]);
        buffer.push_unique(bounds(1));
        buffer.push_unique(Bounds::new(0, 0, 0, 0));
        buffer.push_unique(bounds(3));
        buffer.finish_scope(inner);
        assert_eq!(buffer.current(), &[bounds(2), bounds(1), bounds(3)]);
        buffer.finish_scope(outer);
        assert_eq!(buffer.into_vec(), vec![bounds(1), bounds(2), bounds(3)]);
    }

    #[test]
    fn empty_scopes_leave_parent_unchanged() {
        let mut buffer = DamageBuffer::new(&[bounds(1)]);
        let outer = buffer.enter_scope(&[]);
        let inner = buffer.enter_scope(&[]);
        buffer.finish_scope(inner);
        assert!(buffer.current().is_empty());
        buffer.finish_scope(outer);
        assert_eq!(buffer.into_vec(), vec![bounds(1)]);
    }

    #[test]
    fn scope_indices_survive_growth_and_reuse_allocated_storage() {
        let mut buffer = DamageBuffer::new(&[bounds(-1)]);
        let outer = buffer.enter_scope(&[]);
        let inner = buffer.enter_scope(&[]);
        for x in 0..1024 {
            buffer.push_unique(bounds(x));
        }
        buffer.finish_scope(inner);
        buffer.finish_scope(outer);
        let expected = buffer.current().to_vec();
        // Reserve once for another equally sized independent input. Sibling
        // propagation must then reuse this storage instead of allocating a Vec.
        buffer.values.reserve(1024);
        let pointer = buffer.values.as_ptr();
        let capacity = buffer.values.capacity();
        for _ in 0..4 {
            let sibling = buffer.enter_scope(&[]);
            for x in 0..1024 {
                buffer.push_unique(bounds(x));
            }
            buffer.finish_scope(sibling);
            assert_eq!(buffer.current(), expected);
            assert_eq!(buffer.values.as_ptr(), pointer);
            assert_eq!(buffer.values.capacity(), capacity);
        }
    }
}
