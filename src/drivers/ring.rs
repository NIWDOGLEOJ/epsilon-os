//! Fixed-capacity event ring for interrupt-to-task handoff.
//!
//! Interrupt handlers must never allocate. The global allocator is a plain
//! spinlock, so an ISR that allocates while the code it interrupted is itself
//! inside the allocator spins forever on a lock that can no longer be released --
//! the machine hangs hard. `Vec` and `VecDeque` grow on push, so neither is safe
//! to fill from an ISR.
//!
//! `EventRing` is fully preallocated in the static itself, so pushing from an
//! interrupt handler touches no allocator. A push into a full ring drops the
//! newest event rather than growing.

/// Ring buffer of `N` `Copy` events, preallocated and never resized.
pub struct EventRing<T: Copy, const N: usize> {
    slots: [Option<T>; N],
    head: usize,
    len: usize,
}

impl<T: Copy, const N: usize> EventRing<T, N> {
    pub const fn new() -> Self {
        Self {
            slots: [None; N],
            head: 0,
            len: 0,
        }
    }

    /// Appends an event. Returns `false` if the ring was full and the event was
    /// dropped -- the only sane response in an ISR, which cannot block or grow.
    pub fn push(&mut self, value: T) -> bool {
        if self.len == N {
            return false;
        }
        self.slots[(self.head + self.len) % N] = Some(value);
        self.len += 1;
        true
    }

    /// Removes and returns the oldest event.
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let value = self.slots[self.head].take();
        self.head = (self.head + 1) % N;
        self.len -= 1;
        value
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<T: Copy + PartialEq, const N: usize> EventRing<T, N> {
    /// True if `value` is currently queued.
    pub fn contains(&self, value: &T) -> bool {
        (0..self.len).any(|i| self.slots[(self.head + i) % N] == Some(*value))
    }
}
