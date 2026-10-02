use super::*;
use std::{cell::RefCell, rc::Rc};

fn resource(id: u32, size: (u32, u32)) -> SceneResources<u32> {
    SceneResources {
        target_size: size,
        allocation: id,
    }
}

#[test]
fn siblings_cannot_reuse_an_unsubmitted_allocation() {
    let mut pool = SceneResourcePool::default();
    pool.recycle(resource(1, (17, 19)));
    pool.recycle(resource(2, (17, 19)));
    assert!(pool.acquire((17, 19)).is_none());
    assert_eq!(pool.pending().len(), 2);
    pool.begin_frame();
    assert_eq!(pool.acquire((17, 19)).unwrap().allocation, 2);
    assert_eq!(pool.acquire((17, 19)).unwrap().allocation, 1);
    assert!(pool.acquire((17, 19)).is_none());
    assert!(pool.pending().is_empty());
}

#[test]
fn exact_size_reuse_preserves_the_unmatched_lifo_fallback() {
    let mut pool = SceneResourcePool::default();
    for (id, size) in [(1, (8, 8)), (2, (17, 19)), (3, (8, 8)), (4, (32, 64))] {
        pool.recycle(resource(id, size));
    }
    pool.begin_frame();
    for (id, size) in [(2, (17, 19)), (4, (32, 64)), (3, (8, 8)), (1, (8, 8))] {
        let acquired = pool.acquire(size).unwrap();
        assert_eq!(acquired.allocation, id);
        assert_eq!(acquired.target_size, size);
    }
}

#[test]
fn a_size_miss_retargets_the_existing_allocation_without_replacing_it() {
    let mut pool = SceneResourcePool::default();
    pool.recycle(resource(7, (17, 19)));
    pool.begin_frame();
    let acquired = pool.acquire((33, 31)).unwrap();
    assert_eq!(acquired.allocation, 7);
    assert_eq!(acquired.target_size, (33, 31));
    assert!(pool.available().is_empty());
}

#[test]
fn newly_recycled_resources_do_not_hide_older_available_allocations() {
    let mut pool = SceneResourcePool::default();
    pool.recycle(resource(1, (17, 19)));
    pool.begin_frame();
    pool.recycle(resource(2, (17, 19)));
    assert_eq!(pool.acquire((17, 19)).unwrap().allocation, 1);
    assert!(pool.acquire((17, 19)).is_none());
    pool.begin_frame();
    assert_eq!(pool.acquire((17, 19)).unwrap().allocation, 2);
}

#[test]
fn clearing_the_pool_drops_both_available_and_pending_ownership() {
    struct Allocation(u32, Rc<RefCell<Vec<u32>>>);
    impl Drop for Allocation {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }
    let dropped = Rc::new(RefCell::new(Vec::new()));
    let mut pool = SceneResourcePool::default();
    pool.recycle(SceneResources {
        target_size: (17, 19),
        allocation: Allocation(1, dropped.clone()),
    });
    pool.begin_frame();
    pool.recycle(SceneResources {
        target_size: (17, 19),
        allocation: Allocation(2, dropped.clone()),
    });
    pool.clear();
    let mut ids = dropped.borrow().clone();
    ids.sort_unstable();
    assert_eq!(ids, [1, 2]);
    assert!(pool.available().is_empty());
    assert!(pool.pending().is_empty());
}
