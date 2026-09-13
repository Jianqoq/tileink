use super::*;

#[test]
fn distinct_buffers_have_independent_slots_even_for_the_same_shader_stage() {
    let mut writes = UniformWrites::default();
    assert_eq!(writes.write(&11, 16, 256, 2, &[1; 16]), 0);
    assert_eq!(writes.write(&22, 16, 256, 2, &[2; 16]), 0);
    assert_eq!(writes.write(&11, 16, 256, 2, &[3; 16]), 256);
    let uploaded: std::collections::BTreeMap<_, _> =
        writes.iter().map(|(&id, bytes)| (id, bytes)).collect();
    assert_eq!(uploaded.len(), 2);
    assert_eq!(&uploaded[&11][..16], &[1; 16]);
    assert_eq!(&uploaded[&11][256..], &[3; 16]);
    assert_eq!(uploaded[&22], &[2u8; 16]);
}

#[test]
fn cloned_resource_handles_refer_to_the_same_allocation() {
    let first = std::rc::Rc::new(11);
    let alias = first.clone();
    let mut writes = UniformWrites::default();
    assert_eq!(writes.write(&first, 16, 256, 2, &[1; 16]), 0);
    assert_eq!(writes.write(&alias, 16, 256, 2, &[2; 16]), 256);
    assert_eq!(writes.iter().count(), 1);
}

#[test]
fn rollover_only_depends_on_the_requested_resource() {
    let mut writes = UniformWrites::default();
    writes.write(&11, 16, 256, 1, &[1; 16]);
    assert!(writes.is_full(&11));
    assert!(!writes.is_full(&22));
    writes.write(&22, 16, 256, 2, &[2; 16]);
    assert!(!writes.is_full(&22));
    writes.clear();
    assert!(!writes.is_full(&11));
    assert_eq!(writes.write(&11, 16, 256, 1, &[3; 16]), 0);
}

#[test]
fn unwritten_bytes_and_alignment_padding_remain_zero() {
    let mut writes = UniformWrites::default();
    writes.write(&1, 16, 256, 2, &[1; 4]);
    writes.write(&1, 16, 256, 2, &[2; 4]);
    let (_, bytes) = writes.iter().next().unwrap();
    assert_eq!(&bytes[4..256], &[0; 252]);
    assert_eq!(&bytes[260..], &[0; 12]);
}

#[test]
#[should_panic(expected = "submit before")]
fn writing_a_full_resource_without_submission_is_rejected() {
    let mut writes = UniformWrites::default();
    writes.write(&1, 16, 256, 1, &[1; 16]);
    writes.write(&1, 16, 256, 1, &[2; 16]);
}

#[test]
#[should_panic(expected = "layout changed")]
fn one_allocation_cannot_change_its_uniform_layout_inside_a_batch() {
    let mut writes = UniformWrites::default();
    writes.write(&1, 16, 256, 2, &[1; 16]);
    writes.write(&1, 32, 256, 2, &[2; 16]);
}

#[test]
#[should_panic(expected = "invalid uniform layout")]
fn zero_slot_layout_is_rejected() {
    UniformWrites::default().write(&1, 16, 256, 0, &[1; 16]);
}

#[test]
#[should_panic(expected = "uniform buffer size overflow")]
fn overflowing_layout_is_rejected_before_allocation() {
    UniformWrites::default().write(&1, 16, u64::MAX, 2, &[1; 16]);
}

#[test]
fn hash_collisions_do_not_merge_different_allocations() {
    #[derive(Clone, PartialEq, Eq)]
    struct Buffer(u64);
    impl std::hash::Hash for Buffer {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            state.write_u64(0);
        }
    }
    let mut writes = UniformWrites::default();
    assert_eq!(writes.write(&Buffer(11), 16, 256, 1, &[1; 16]), 0);
    assert_eq!(writes.write(&Buffer(22), 16, 256, 2, &[2; 16]), 0);
    assert!(writes.is_full(&Buffer(11)));
    assert!(!writes.is_full(&Buffer(22)));
    let uploaded: std::collections::BTreeMap<_, _> = writes
        .iter()
        .map(|(buffer, bytes)| (buffer.0, bytes))
        .collect();
    assert_eq!(uploaded[&11], &[1; 16]);
    assert_eq!(uploaded[&22], &[2; 16]);
}

#[test]
fn small_batches_do_not_hash_resource_handles() {
    use std::{cell::Cell, rc::Rc};

    #[derive(Clone)]
    struct Buffer(u64, Rc<Cell<usize>>);
    impl PartialEq for Buffer {
        fn eq(&self, other: &Self) -> bool {
            self.0 == other.0
        }
    }
    impl Eq for Buffer {}
    impl Hash for Buffer {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.1.set(self.1.get() + 1);
            self.0.hash(state);
        }
    }

    // Tiny batches must not pay for a hash index on either capacity checks or
    // writes. This counts actual work, rather than asserting the representation.
    let hashes = Rc::new(Cell::new(0));
    let buffers: Vec<_> = (0..3).map(|id| Buffer(id, hashes.clone())).collect();
    let mut writes = UniformWrites::default();
    for slot in 0..4 {
        for buffer in &buffers {
            assert!(!writes.is_full(buffer));
            assert_eq!(writes.write(buffer, 4, 16, 4, &[slot as u8; 4]), slot * 16);
        }
    }
    assert!(buffers.iter().all(|buffer| writes.is_full(buffer)));
    assert_eq!(writes.iter().count(), 3);
    assert_eq!(hashes.get(), 0);
}

#[test]
fn growing_and_clearing_batches_preserve_colliding_resource_slots() {
    #[derive(Clone, PartialEq, Eq)]
    struct Buffer(u64);
    impl Hash for Buffer {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            state.write_u64(0);
        }
    }

    // Cross the small-batch boundary while existing buffers already contain
    // data, revisit every buffer after growth, then reuse the cleared batch.
    let mut writes = UniformWrites::default();
    for id in 0..65 {
        assert_eq!(writes.write(&Buffer(id), 4, 16, 2, &[id as u8; 4]), 0);
        assert!(!writes.is_full(&Buffer(0)));
    }
    for id in (0..65).rev() {
        assert_eq!(
            writes.write(&Buffer(id), 4, 16, 2, &[255 - id as u8; 4]),
            16
        );
        assert!(writes.is_full(&Buffer(id)));
    }
    let uploaded: std::collections::BTreeMap<_, _> = writes
        .iter()
        .map(|(buffer, bytes)| (buffer.0, bytes.to_vec()))
        .collect();
    assert_eq!(uploaded.len(), 65);
    for (id, bytes) in uploaded {
        assert_eq!(&bytes[..4], &[id as u8; 4]);
        assert_eq!(&bytes[4..16], &[0; 12]);
        assert_eq!(&bytes[16..], &[255 - id as u8; 4]);
    }
    writes.clear();
    assert_eq!(writes.iter().count(), 0);
    for id in [64, 0, 66] {
        assert!(!writes.is_full(&Buffer(id)));
        assert_eq!(writes.write(&Buffer(id), 4, 16, 1, &[9; 4]), 0);
    }
    assert_eq!(writes.iter().count(), 3);
    assert!(writes.iter().all(|(_, bytes)| bytes == [9; 4]));
    for id in 100..120 {
        assert_eq!(writes.write(&Buffer(id), 4, 16, 1, &[7; 4]), 0);
    }
    // Re-enter indexed lookup after clearing: no previous-only key may resolve
    // to an arena in this batch, even though its numeric position was reused.
    for id in 1..64 {
        assert!(!writes.is_full(&Buffer(id)));
    }
    assert_eq!(writes.write(&Buffer(1), 4, 16, 1, &[8; 4]), 0);
    assert_eq!(writes.iter().count(), 24);
}
