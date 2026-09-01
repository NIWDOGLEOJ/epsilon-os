//! Kernel Dynamic Heap Allocator for AegisOS
//!
//! Provides a 16MB kernel heap at virtual address `0xFFFF_9000_0000_0000`,
//! enabling `extern crate alloc;` and Rust dynamic collections.

use linked_list_allocator::LockedHeap;
use super::frame::{alloc_frame, PhysAddr, PAGE_SIZE};
use super::paging::{map_page, PageTableFlags, VirtAddr};

pub const HEAP_START: u64 = 0xFFFF_9000_0000_0000;
pub const HEAP_SIZE: usize = 16 * 1024 * 1024; // 16 MB
pub const HEAP_FRAME_COUNT: usize = HEAP_SIZE / PAGE_SIZE; // 4096 frames

/// Thread-safe global allocator singleton enabling dynamic heap collections.
#[global_allocator]
pub static GLOBAL_ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Initializes the 16MB kernel heap by allocating physical frames and mapping them into the kernel PML4.
///
/// # Safety
/// Must be invoked after `frame::init()` and `paging::init()`.
pub unsafe fn init_heap(master_kernel_pml4_phys: PhysAddr) {
    // 1. Allocate 4096 physical frames and map them to [HEAP_START .. HEAP_START + 16MB]
    for frame_idx in 0..HEAP_FRAME_COUNT {
        let virt = VirtAddr::new(HEAP_START + (frame_idx as u64) * (PAGE_SIZE as u64));
        let phys = alloc_frame().expect("Fatal: Out of physical memory while creating kernel heap");

        map_page(
            master_kernel_pml4_phys,
            virt,
            phys,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE,
        );
    }

    // 2. Initialize the global heap allocator with the mapped virtual memory buffer
    GLOBAL_ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
}
