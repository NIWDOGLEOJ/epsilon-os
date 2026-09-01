//! Physical Memory Frame Allocator for AegisOS
//!
//! Manages 4GB of physical memory using a 128KB bitmap allocator.

use spin::Mutex;

/// Physical address wrapper ensuring 64-bit alignment and type safety.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysAddr(pub u64);

impl PhysAddr {
    #[inline(always)]
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    #[inline(always)]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub const fn is_null(&self) -> bool {
        self.0 == 0
    }

    #[inline(always)]
    pub const fn is_aligned_4k(&self) -> bool {
        (self.0 & 0xFFF) == 0
    }

    #[inline(always)]
    pub const fn align_down_4k(&self) -> Self {
        Self(self.0 & !0xFFF)
    }

    #[inline(always)]
    pub const fn align_up_4k(&self) -> Self {
        Self((self.0 + 0xFFF) & !0xFFF)
    }
}

pub const PAGE_SIZE: usize = 4096;
pub const MAX_PHYSICAL_MEMORY: u64 = 4 * 1024 * 1024 * 1024; // 4 GB
pub const TOTAL_FRAME_COUNT: usize = (MAX_PHYSICAL_MEMORY / PAGE_SIZE as u64) as usize; // 1,048,576
pub const BITMAP_WORD_COUNT: usize = TOTAL_FRAME_COUNT / 64; // 16,384 words (128 KB)

/// 128KB static storage for the physical frame bitmap in BSS.
/// 1 = Allocated / Reserved, 0 = Free / Usable.
static mut BITMAP_STORAGE: [u64; BITMAP_WORD_COUNT] = [!0u64; BITMAP_WORD_COUNT];

/// Thread-safe Physical Frame Allocator structure.
pub struct BitmapFrameAllocator {
    total_usable_frames: usize,
    allocated_frames: usize,
    last_searched_word: usize,
    hhdm_offset: u64,
}

impl BitmapFrameAllocator {
    /// Creates an uninitialized allocator instance.
    const fn new() -> Self {
        Self {
            total_usable_frames: 0,
            allocated_frames: 0,
            last_searched_word: 0,
            hhdm_offset: 0,
        }
    }

    /// Initializes the bitmap allocator using Limine memory map entries.
    ///
    /// # Safety
    /// Must be called once during early kernel initialization on the BSP.
    pub unsafe fn init(&mut self, memmap_entries: &[&limine::memory_map::Entry], hhdm_offset: u64) {
        self.hhdm_offset = hhdm_offset;
        let bitmap_ptr = &raw mut BITMAP_STORAGE;
        
        // 1. Mark all frames as allocated (1) initially
        for i in 0..BITMAP_WORD_COUNT {
            (*bitmap_ptr)[i] = !0u64;
        }

        let mut usable_count = 0;

        // 2. Clear bits for usable RAM regions
        for entry in memmap_entries {
            if entry.entry_type == limine::memory_map::EntryType::USABLE {
                let start_addr = entry.base;
                let end_addr = (entry.base + entry.length).min(MAX_PHYSICAL_MEMORY);

                let start_frame = (start_addr / PAGE_SIZE as u64) as usize;
                let end_frame = (end_addr / PAGE_SIZE as u64) as usize;

                for frame_idx in start_frame..end_frame {
                    // Frame 0 is preserved as allocated to avoid null physical address
                    if frame_idx == 0 {
                        continue;
                    }
                    let word_idx = frame_idx / 64;
                    let bit_idx = frame_idx % 64;

                    if word_idx < BITMAP_WORD_COUNT {
                        (*bitmap_ptr)[word_idx] &= !(1u64 << bit_idx);
                        usable_count += 1;
                    }
                }
            }
        }

        self.total_usable_frames = usable_count;
        self.allocated_frames = 0;
        self.last_searched_word = 0;
    }

    /// Allocates a single 4KB physical frame.
    /// Returns `Some(PhysAddr)` on success, or `None` if physical memory is exhausted.
    pub fn alloc_frame(&mut self) -> Option<PhysAddr> {
        let start_word = self.last_searched_word;
        let bitmap_ptr = &raw mut BITMAP_STORAGE;

        for offset in 0..BITMAP_WORD_COUNT {
            let word_idx = (start_word + offset) % BITMAP_WORD_COUNT;
            let word = unsafe { (*bitmap_ptr)[word_idx] };

            if word != !0u64 {
                let free_bit = (!word).trailing_zeros() as usize;
                unsafe {
                    (*bitmap_ptr)[word_idx] |= 1u64 << free_bit;
                }
                self.allocated_frames += 1;
                self.last_searched_word = word_idx;

                let frame_idx = word_idx * 64 + free_bit;
                let phys = (frame_idx as u64) * (PAGE_SIZE as u64);
                return Some(PhysAddr::new(phys));
            }
        }

        None // Out of physical memory
    }

    /// Allocates a single 4KB physical frame and clears its contents to zero.
    pub fn alloc_zeroed_frame(&mut self) -> Option<PhysAddr> {
        let frame = self.alloc_frame()?;
        let virt_addr = frame.as_u64() + self.hhdm_offset;
        unsafe {
            core::ptr::write_bytes(virt_addr as *mut u8, 0, PAGE_SIZE);
        }
        Some(frame)
    }

    /// Frees a previously allocated 4KB physical frame.
    pub fn free_frame(&mut self, frame: PhysAddr) {
        if !frame.is_aligned_4k() || frame.as_u64() >= MAX_PHYSICAL_MEMORY || frame.is_null() {
            return;
        }

        let frame_idx = (frame.as_u64() / PAGE_SIZE as u64) as usize;
        let word_idx = frame_idx / 64;
        let bit_idx = frame_idx % 64;
        let bitmap_ptr = &raw mut BITMAP_STORAGE;

        if word_idx < BITMAP_WORD_COUNT {
            let is_allocated = unsafe { ((*bitmap_ptr)[word_idx] & (1u64 << bit_idx)) != 0 };
            if is_allocated {
                unsafe {
                    (*bitmap_ptr)[word_idx] &= !(1u64 << bit_idx);
                }
                if self.allocated_frames > 0 {
                    self.allocated_frames -= 1;
                }
                if word_idx < self.last_searched_word {
                    self.last_searched_word = word_idx;
                }
            }
        }
    }

    /// Returns `(used_bytes, total_usable_bytes)`.
    pub fn get_memory_stats(&self) -> (u64, u64) {
        let used_bytes = (self.allocated_frames as u64) * (PAGE_SIZE as u64);
        let total_bytes = (self.total_usable_frames as u64) * (PAGE_SIZE as u64);
        (used_bytes, total_bytes)
    }
}

/// Global synchronized Physical Frame Allocator.
pub static GLOBAL_FRAME_ALLOCATOR: Mutex<BitmapFrameAllocator> =
    Mutex::new(BitmapFrameAllocator::new());

/// Global API: Allocates a 4KB physical frame.
pub fn alloc_frame() -> Option<PhysAddr> {
    GLOBAL_FRAME_ALLOCATOR.lock().alloc_frame()
}

/// Global API: Allocates a zeroed 4KB physical frame.
pub fn alloc_zeroed_frame() -> Option<PhysAddr> {
    GLOBAL_FRAME_ALLOCATOR.lock().alloc_zeroed_frame()
}

/// Global API: Frees a 4KB physical frame.
pub fn free_frame(frame: PhysAddr) {
    GLOBAL_FRAME_ALLOCATOR.lock().free_frame(frame);
}

/// Global API: Returns `(used_bytes, total_usable_bytes)`.
pub fn get_memory_stats() -> (u64, u64) {
    GLOBAL_FRAME_ALLOCATOR.lock().get_memory_stats()
}
