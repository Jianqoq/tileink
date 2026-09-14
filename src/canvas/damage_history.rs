//! Root-space damage events are independent of prunable node-state overlays.
//! Replaying resolved bounds as local damage would apply enclosing filters twice.
use super::{Bounds, RetainedNodeId};
use std::{borrow::Cow, collections::HashSet, rc::Rc};

const MAX_STEPS: u16 = 256;

#[derive(Clone, Debug)]
pub(crate) struct ResolvedDamage {
    // Clipping may discard most sources. Exact-length storage prevents retained
    // histories from keeping spare capacity from large transient work buffers.
    pub(crate) bounds: Box<[Bounds]>,
    pub(crate) dirty_backdrops: Rc<[RetainedNodeId]>,
}

#[cfg(test)]
impl ResolvedDamage {
    fn bounds_storage_capacity(&self) -> usize {
        self.bounds.len()
    }
}

#[derive(Clone, Debug)]
struct DamageStep {
    from: u64,
    to: u64,
    depth: u16,
    // The shared event owns its bounds and payload. Immediate rendering borrows
    // them, avoiding an extra payload Rc and a Vec-to-Rc bounds allocation.
    damage: ResolvedDamage,
    previous: Option<Rc<Self>>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DamageHistory {
    scoped: bool,
    floor: u64,
    tail: Option<Rc<DamageStep>>,
}

impl DamageHistory {
    pub(crate) fn initial(scoped: bool, version: u64) -> Self {
        Self {
            scoped,
            floor: if scoped { version } else { 0 },
            tail: None,
        }
    }

    pub(crate) fn is_scoped(&self) -> bool {
        self.scoped
    }

    pub(crate) fn advance(
        &self,
        scoped: bool,
        from: u64,
        to: u64,
        damage: Option<ResolvedDamage>,
    ) -> Self {
        if !scoped {
            // Remember a domain transition after dropping its events. A renderer
            // that has not consumed the old domain must restore its root history.
            return if self.scoped {
                Self {
                    scoped,
                    floor: to,
                    tail: None,
                }
            } else {
                self.clone()
            };
        }
        let Some(damage) = damage else {
            return Self::initial(true, to);
        };
        let previous = self
            .tail
            .as_ref()
            .filter(|step| step.to == from && step.depth < MAX_STEPS)
            .cloned();
        let floor = if previous.is_some() { self.floor } else { from };
        let depth = previous.as_ref().map_or(1, |step| step.depth + 1);
        Self {
            scoped,
            floor,
            tail: Some(Rc::new(DamageStep {
                from,
                to,
                depth,
                damage,
                previous,
            })),
        }
    }

    /// None keeps the root-domain diff. Err requires recovery before any node Rc
    /// shortcut. Immediate frames borrow the event; skipped frames own their merged
    /// result. Callers that retain a result independently can request an owned copy.
    pub(crate) fn resolve(
        &self,
        from: u64,
        to: u64,
    ) -> Result<Option<Cow<'_, ResolvedDamage>>, ()> {
        if from < self.floor || from > to {
            return Err(());
        }
        if !self.scoped {
            return Ok(None);
        }
        if from == to {
            return Ok(Some(Cow::Owned(ResolvedDamage {
                bounds: Box::new([]),
                dirty_backdrops: Rc::new([]),
            })));
        }
        let tail = self.tail.as_ref().filter(|step| step.to == to).ok_or(())?;
        if tail.from == from {
            return Ok(Some(Cow::Borrowed(&tail.damage)));
        }
        let mut bounds = Vec::new();
        let mut dirty_backdrops = HashSet::new();
        let mut cursor = Some(tail.as_ref());
        let mut expected = to;
        while let Some(step) = cursor {
            if step.to != expected || step.from < from {
                return Err(());
            }
            bounds.extend_from_slice(&step.damage.bounds);
            dirty_backdrops.extend(step.damage.dirty_backdrops.iter().copied());
            if step.from == from {
                return Ok(Some(Cow::Owned(ResolvedDamage {
                    bounds: bounds.into_boxed_slice(),
                    dirty_backdrops: dirty_backdrops.into_iter().collect::<Vec<_>>().into(),
                })));
            }
            expected = step.from;
            cursor = step.previous.as_deref();
        }
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn damage(x: i32, owner: u64) -> ResolvedDamage {
        ResolvedDamage {
            bounds: vec![Bounds::new(x, 0, x + 1, 1)].into_boxed_slice(),
            dirty_backdrops: vec![RetainedNodeId::for_owner(owner)].into(),
        }
    }
    #[test]
    fn skipped_updates_union_resolved_output_without_replaying_it() {
        let history = DamageHistory::initial(true, 1)
            .advance(true, 1, 2, Some(damage(16, 10)))
            .advance(true, 2, 3, Some(damage(32, 11)));
        let result = history.resolve(1, 3).unwrap().unwrap();
        assert_eq!(
            result.bounds.as_ref(),
            &[Bounds::new(32, 0, 33, 1), Bounds::new(16, 0, 17, 1)]
        );
        assert_eq!(result.dirty_backdrops.len(), 2);
        assert_eq!(
            history.resolve(2, 3).unwrap().unwrap().bounds.as_ref(),
            &[Bounds::new(32, 0, 33, 1)]
        );
    }
    #[test]
    fn event_epochs_preserve_the_immediate_partial_path() {
        let mut history = DamageHistory::initial(true, 1);
        for from in 1..=u64::from(MAX_STEPS) + 1 {
            history = history.advance(true, from, from + 1, Some(damage(from as i32, 10)));
            assert!(history.resolve(from, from + 1).unwrap().is_some());
        }
        assert!(history.resolve(1, u64::from(MAX_STEPS) + 2).is_err());
    }
    #[test]
    fn domain_exit_and_incomplete_steps_require_recovery_only_across_the_boundary() {
        let history = DamageHistory::initial(true, 1).advance(true, 1, 2, None);
        assert!(history.resolve(1, 2).is_err());
        let history = history.advance(true, 2, 3, Some(damage(0, 10)));
        assert!(history.resolve(2, 3).unwrap().is_some());
        let history = history
            .advance(false, 3, 4, None)
            .advance(false, 4, 5, None);
        assert!(history.resolve(3, 5).is_err());
        assert!(history.resolve(4, 5).unwrap().is_none());
    }
    #[test]
    fn invisible_output_keeps_its_dirty_backdrop_event() {
        let history = DamageHistory::initial(true, 1).advance(
            true,
            1,
            2,
            Some(ResolvedDamage {
                bounds: Box::new([]),
                dirty_backdrops: vec![RetainedNodeId::for_owner(10)].into(),
            }),
        );
        let result = history.resolve(1, 2).unwrap().unwrap();
        assert!(result.bounds.is_empty());
        assert_eq!(result.dirty_backdrops.len(), 1);
    }

    #[test]
    fn resolved_events_outlive_history_updates_and_forks() {
        let first = DamageHistory::initial(true, 1).advance(true, 1, 2, Some(damage(16, 10)));
        let held = (*first.resolve(1, 2).unwrap().unwrap()).clone();
        let left = first.advance(true, 2, 3, Some(damage(32, 11)));
        let right = first.advance(true, 2, 3, Some(damage(48, 12)));
        drop(first);
        let left_event = (*left.resolve(2, 3).unwrap().unwrap()).clone();
        let right_event = (*right.resolve(2, 3).unwrap().unwrap()).clone();
        drop(left);
        drop(right);
        assert_eq!(&held.bounds[..], &[Bounds::new(16, 0, 17, 1)]);
        assert_eq!(&left_event.bounds[..], &[Bounds::new(32, 0, 33, 1)]);
        assert_eq!(&right_event.bounds[..], &[Bounds::new(48, 0, 49, 1)]);
        assert_eq!(
            held.dirty_backdrops.as_ref(),
            &[RetainedNodeId::for_owner(10)]
        );
        assert_eq!(
            left_event.dirty_backdrops.as_ref(),
            &[RetainedNodeId::for_owner(11)]
        );
        assert_eq!(
            right_event.dirty_backdrops.as_ref(),
            &[RetainedNodeId::for_owner(12)]
        );
    }

    #[test]
    fn immediate_resolution_shares_event_bounds() {
        let history = DamageHistory::initial(true, 1).advance(true, 1, 2, Some(damage(16, 10)));
        let original = history.tail.as_ref().unwrap().damage.bounds.as_ptr();
        let resolved = history.resolve(1, 2).unwrap().unwrap();
        assert_eq!(resolved.bounds.as_ptr(), original);
        let unchanged = history.resolve(2, 2).unwrap().unwrap();
        assert!(unchanged.bounds.is_empty());
        assert!(unchanged.dirty_backdrops.is_empty());
    }

    #[test]
    fn clipped_damage_history_does_not_retain_empty_source_capacity() {
        let canvas = crate::Canvas::new(64, 64, 1.0);
        let mut pending = crate::canvas::RetainedDamage::default();
        for x in 128..1152 {
            pending.add_unattributed(Bounds::new(x * 2, 0, x * 2 + 1, 1));
        }
        let mut history = DamageHistory::initial(true, 1);
        for from in 1..=4 {
            let propagated = canvas.propagate_damage(&pending);
            assert!(propagated.bounds.is_empty());
            assert!(propagated.bounds.capacity() >= 1024);
            history = history.advance(
                true,
                from,
                from + 1,
                Some(ResolvedDamage {
                    bounds: propagated.bounds.into(),
                    dirty_backdrops: Rc::new([]),
                }),
            );
            // Root clipping can discard all sources. Keep only delivered bounds,
            // rather than holding a large empty work buffer in each history event.
            let mut cursor = history.tail.as_deref();
            while let Some(step) = cursor {
                assert_eq!(step.damage.bounds_storage_capacity(), 0);
                cursor = step.previous.as_deref();
            }
        }
    }
}
