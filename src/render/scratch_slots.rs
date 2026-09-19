/// API-independent scratch leases. Physical surfaces and displaced allocations
/// stay with the backend; slot selection and frame/context reset use one policy.
#[derive(Default)]
pub(crate) struct ScratchSlots {
    occupied: Vec<bool>,
}

impl ScratchSlots {
    pub(crate) fn reset(&mut self, count: usize) {
        self.occupied.clear();
        self.occupied.resize(count, false);
    }

    pub(crate) fn acquire(&mut self) -> Option<usize> {
        let index = self.occupied.iter().position(|used| !used)?;
        self.occupied[index] = true;
        Some(index)
    }

    // Native surface pools grow live leases; wgpu sizes its slots before acquiring.
    #[cfg(any(test, feature = "dx12", feature = "vulkan"))]
    pub(crate) fn push_occupied(&mut self) -> usize {
        let index = self.occupied.len();
        self.occupied.push(true);
        index
    }

    pub(crate) fn occupy(&mut self, index: usize) {
        self.occupied[index] = true;
    }

    pub(crate) fn release(&mut self, index: usize) {
        self.occupied[index] = false;
    }

    #[cfg(any(test, feature = "dx12", feature = "vulkan"))]
    pub(crate) fn is_occupied(&self, index: usize) -> bool {
        self.occupied.get(index).copied().unwrap_or(false)
    }

    pub(crate) fn any_occupied(&self) -> bool {
        self.occupied.iter().any(|used| *used)
    }

    pub(crate) fn release_all(&mut self) {
        self.occupied.fill(false);
    }
}

#[cfg(test)]
mod tests {
    use super::ScratchSlots;

    #[test]
    fn scratch_leases_reuse_first_free_without_aliasing_live_slots() {
        let mut slots = ScratchSlots::default();
        slots.reset(3);
        assert_eq!(slots.acquire(), Some(0));
        assert_eq!(slots.acquire(), Some(1));
        assert_eq!(slots.acquire(), Some(2));
        assert_eq!(slots.acquire(), None);
        slots.release(1);
        assert_eq!(slots.acquire(), Some(1));
        assert!(slots.is_occupied(0) && slots.is_occupied(2));
        assert!(!slots.is_occupied(3));
        assert_eq!(slots.push_occupied(), 3);
        assert_eq!(slots.acquire(), None);
    }

    #[test]
    fn scratch_transfer_and_context_reset_preserve_lease_state() {
        let mut slots = ScratchSlots::default();
        slots.reset(2);
        slots.occupy(1);
        assert_eq!(slots.acquire(), Some(0));
        slots.release_all();
        assert!(!slots.any_occupied());
        assert_eq!(slots.acquire(), Some(0));
        slots.reset(1);
        assert!(!slots.any_occupied());
        assert_eq!(slots.acquire(), Some(0));
        assert_eq!(slots.acquire(), None);
        slots.reset(0);
        assert!(!slots.any_occupied());
        assert_eq!(slots.acquire(), None);
    }
}
