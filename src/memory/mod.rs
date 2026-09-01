//! Memory Management Subsystem for AegisOS
//!
//! Exposes physical frame allocation, dynamic heap management, and 4-level PML4 paging.

pub mod frame;
pub mod heap;
pub mod paging;

pub use frame::{
    alloc_frame, alloc_zeroed_frame, free_frame, get_memory_stats, BitmapFrameAllocator,
    PhysAddr, BITMAP_WORD_COUNT, MAX_PHYSICAL_MEMORY, PAGE_SIZE, TOTAL_FRAME_COUNT,
};

pub use heap::{
    init_heap, GLOBAL_ALLOCATOR, HEAP_FRAME_COUNT, HEAP_SIZE, HEAP_START,
};

pub use paging::{
    create_user_address_space, destroy_user_address_space, flush_tlb, get_kernel_pml4,
    init_paging, map_page, phys_to_virt, read_cr3, translate_addr, unmap_page, virt_to_phys,
    write_cr3, PageTable, PageTableEntry, PageTableFlags, VirtAddr,
};

/// Master initialization routine for the AegisOS Memory Subsystem.
///
/// # Safety
/// Must be called during early boot before any heap allocations or multitasking.
pub unsafe fn init(memmap_entries: &[&limine::memory_map::Entry], hhdm_offset: u64) {
    // 1. Initialize 128KB Bitmap Physical Frame Allocator
    frame::GLOBAL_FRAME_ALLOCATOR
        .lock()
        .init(memmap_entries, hhdm_offset);

    // 2. Initialize Paging and save Kernel PML4
    paging::init_paging(hhdm_offset);

    // 3. Initialize 16MB Kernel Dynamic Heap
    let kernel_pml4 = paging::get_kernel_pml4();
    heap::init_heap(kernel_pml4);
}
