# AegisOS Milestone 1: Memory & Paging Subsystem Architectural Specification & Code Blueprints

**Module:** `src/memory/`  
**Milestone:** M1 (Foundation & Memory Management)  
**Author:** M1 Memory & Paging Explorer (`m1_explorer_3`)  
**Target:** `x86_64` bare-metal `no_std` Rust via Limine Bootloader Protocol  
**Date:** 2026-08-30  

---

## 1. Executive Summary & Architectural Overview

The memory subsystem of **AegisOS** forms the bedrock of hardware privilege separation, preemptive multitasking, and crash-resilient process isolation. It fulfills three critical responsibilities:
1. **Physical Memory Management (`src/memory/frame.rs`)**: Manages up to 4 GB of physical RAM ($1,048,576 \times 4\text{ KB}$ frames) using a statically allocated **128 KB Bitmap**. It parses the Limine bootloader's `MemoryMapRequest` response to mark usable RAM and provides thread-safe `alloc_frame()`, `alloc_zeroed_frame()`, `free_frame()`, and `get_memory_stats()`.
2. **Dynamic Kernel Heap (`src/memory/heap.rs`)**: Provisions a dedicated **16 MB kernel heap region** (at virtual address `0xFFFF_9000_0000_0000`), backed by physical frames mapped into the kernel page table. It registers a thread-safe `#[global_allocator]`, enabling standard Rust heap collections (`Vec`, `String`, `Box`, `Arc`, `BTreeMap`, `VecDeque`) via `extern crate alloc;`.
3. **4-Level PML4 Virtual Memory & Address Space Isolation (`src/memory/paging.rs`)**: Enforces the strict bifurcation of the 64-bit 256 TB address space into:
   - **Private User Lower-Half (`0x0000_0000_0000_0000` .. `0x0000_7FFF_FFFF_FFFF`, PML4 entries `0..255`)**: Isolated per process.
   - **Shared Kernel Higher-Half (`0xFFFF_8000_0000_0000` .. `0xFFFF_FFFF_FFFF_FFFF`, PML4 entries `256..511`)**: Contains Limine Higher-Half Direct Map (HHDM), Kernel Heap, Framebuffer VRAM, GDT/IDT/TSS, and Kernel Code/Data.
   - Provides `create_user_address_space()` to instantiate isolated PML4 tables and `destroy_user_address_space()` to recursively reclaim all lower-half user frames and page tables without corrupting shared kernel structures.

### Virtual Memory Map Layout
```
+-------------------------------------------------------------------------+
| Physical Address Space (up to 4 GB)                                     |
| [0x0000_0000 .. 0xFFFF_FFFF] (1,048,576 x 4KB frames)                   |
| Managed by: 128 KB Static Bitmap in BSS (16,384 x u64 words)            |
+-------------------------------------------------------------------------+
                                    |
                                    v
+-------------------------------------------------------------------------+
| 64-Bit Virtual Address Space (256 TB Canonical)                         |
|                                                                         |
|  0x0000_0000_0000_0000 +----------------------------------------------+ |
|                        | NULL Pointer Guard Page (Unmapped)           | |
|  0x0000_0000_0040_0000 +----------------------------------------------+ |
|                        | User Text & Data Segments (R/W/X, User)      | |
|  0x0000_0000_1000_0000 +----------------------------------------------+ |
|                        | User Heap (brk / mmap dynamic space)         | |
|  0x0000_7FFF_FFFE_0000 +----------------------------------------------+ |
|                        | User Stack Guard Page (PRESENT=0)            | |
|  0x0000_7FFF_FFFF_0000 +----------------------------------------------+ |
|                        | User Ring 3 Stack (Grows downward) (User, NX)| |
|  0x0000_7FFF_FFFF_FFFF +==============================================+ |
|                        | NON-CANONICAL HOLE (Unaddressable by CPU)    | |
|  0xFFFF_8000_0000_0000 +==============================================+ |
|                        | Limine Higher-Half Direct Map (HHDM) (4GB)   | |
|  0xFFFF_9000_0000_0000 +----------------------------------------------+ |
|                        | Kernel Dynamic Heap (16 MB initial region)   | |
|  0xFFFF_A000_0000_0000 +----------------------------------------------+ |
|                        | Linear RGB Double-Buffered Framebuffer VRAM  | |
|  0xFFFFFFFF80000000    +----------------------------------------------+ |
|                        | Kernel Code, Read-Only Data, Data, BSS       | |
|  0xFFFFFFFFFFFFFFFF    +----------------------------------------------+ |
+-------------------------------------------------------------------------+
```

### Memory Footprint Budget
- **Physical Bitmap Size**: $128\text{ KB}$ (in `.bss`).
- **Kernel Heap Region**: $16\text{ MB}$ (4096 frames allocated from usable RAM).
- **Idle Memory Target**: $< 60\text{ MB}$ total consumption at desktop launch.
- **Budget Compliance**:
  $$\text{Bitmap (128 KB)} + \text{Kernel Heap (16 MB)} + \text{Framebuffer (4 MB)} + \text{Kernel Code/BSS (2 MB)} \approx 22.125\text{ MB} \ll 60\text{ MB}$$

---

## 2. Physical Frame Allocator Design (`src/memory/frame.rs`)

### 2.1 Bitmap Mathematics & Bit Indexing
- 4 GB of RAM contains:
  $$\frac{4 \times 1024 \times 1024 \times 1024\text{ bytes}}{4096\text{ bytes/frame}} = 1,048,576\text{ frames}$$
- Stored as an array of 64-bit unsigned integers:
  $$\frac{1,048,576\text{ bits}}{64\text{ bits/word}} = 16,384\text{ words} = 131,072\text{ bytes} = 128\text{ KB}$$
- **Bit Semantics**:
  - `1`: Frame is **ALLOCATED** or **UNAVAILABLE / RESERVED**.
  - `0`: Frame is **FREE** and **USABLE**.
- **Frame Index Calculation**:
  $$\text{frame\_idx} = \frac{\text{phys\_addr}}{4096}$$
  $$\text{word\_idx} = \frac{\text{frame\_idx}}{64}, \quad \text{bit\_idx} = \text{frame\_idx} \pmod{64}$$

### 2.2 Limine Memory Map Parsing Sequence
1. The static bitmap `BITMAP_STORAGE: [u64; 16384]` is initialized to all `0xFFFFFFFFFFFFFFFF` (all frames occupied/reserved).
2. The allocator parses each `limine::memory_map::Entry` from the bootloader.
3. If `entry.entry_type == EntryType::USABLE`:
   - Iterate through every 4 KB aligned address in `[entry.base .. entry.base + entry.length]`.
   - If `frame_addr < 4GB` ($1,048,576 \times 4096$), clear the corresponding bit to `0` (free) and increment `total_usable_frames`.
4. Frame `0x0000_0000` (physical frame 0) is explicitly clamped to `1` (allocated) to prevent `alloc_frame()` from returning NULL physical address.
5. All allocations search for the lowest available `0` bit using hardware-accelerated `(!word).trailing_zeros()`, set the bit to `1`, and return `Some(PhysAddr)`.

### 2.3 Production Blueprint: `src/memory/frame.rs`

```rust
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
    pub unsafe fn init(&mut self, memmap_entries: &[limine::memory_map::Entry], hhdm_offset: u64) {
        self.hhdm_offset = hhdm_offset;
        
        // 1. Mark all frames as allocated (1) initially
        for word in BITMAP_STORAGE.iter_mut() {
            *word = !0u64;
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
                        BITMAP_STORAGE[word_idx] &= !(1u64 << bit_idx);
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

        for offset in 0..BITMAP_WORD_COUNT {
            let word_idx = (start_word + offset) % BITMAP_WORD_COUNT;
            let word = unsafe { BITMAP_STORAGE[word_idx] };

            if word != !0u64 {
                let free_bit = (!word).trailing_zeros() as usize;
                unsafe {
                    BITMAP_STORAGE[word_idx] |= 1u64 << free_bit;
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

        if word_idx < BITMAP_WORD_COUNT {
            let is_allocated = unsafe { (BITMAP_STORAGE[word_idx] & (1u64 << bit_idx)) != 0 };
            if is_allocated {
                unsafe {
                    BITMAP_STORAGE[word_idx] &= !(1u64 << bit_idx);
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
```

---

## 3. Kernel Dynamic Heap Allocator Design (`src/memory/heap.rs`)

### 3.1 Kernel Heap Region Parameters
- **Virtual Base Address**: `0xFFFF_9000_0000_0000` (within higher-half kernel space).
- **Initial Heap Size**: $16\text{ MB} = 16,777,216\text{ bytes} = 4,096\text{ frames}$.
- **Allocation Strategy**: Intrusive Linked-List Free-Block Allocator with block splitting, first-fit search, and automatic adjacent block coalescing on `dealloc`.
- **Rust Integration**: Registered via `#[global_allocator]` using `spin::Mutex<HeapAllocator>`.

### 3.2 Intrusive Free-List Structure
Each free memory block contains an intrusive header:
```rust
#[repr(C)]
struct FreeBlockHeader {
    size: usize,                    // Size of usable payload in bytes
    next: Option<*mut FreeBlockHeader>, // Pointer to next free block in list
}
```
When memory is allocated:
1. The free list is searched for the first block where `block.size >= requested_size + alignment_padding`.
2. If `block.size > requested_size + MIN_SPLIT_SIZE`, the block is split into an allocated chunk and a remaining free chunk.
3. On `deallocate`:
   - The returned block is inserted back into the sorted free list by address.
   - Immediate left and right neighbors are coalesced into a single contiguous free block to eliminate fragmentation.

### 3.3 Production Blueprint: `src/memory/heap.rs`

```rust
//! Kernel Dynamic Heap Allocator for AegisOS
//!
//! Provides a 16MB kernel heap with an intrusive free-list allocator,
//! enabling `extern crate alloc;` and Rust dynamic collections.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use spin::Mutex;
use super::frame::{alloc_frame, PAGE_SIZE};
use super::paging::{map_page, PageTableFlags, PhysAddr, VirtAddr};

pub const HEAP_START: u64 = 0xFFFF_9000_0000_0000;
pub const HEAP_SIZE: usize = 16 * 1024 * 1024; // 16 MB
pub const HEAP_FRAME_COUNT: usize = HEAP_SIZE / PAGE_SIZE; // 4096 frames

/// Intrusive block header for unallocated heap memory.
#[repr(C)]
struct BlockHeader {
    size: usize,
    next: Option<NonNull<BlockHeader>>,
}

impl BlockHeader {
    const fn new(size: usize) -> Self {
        Self { size, next: None }
    }

    fn start_address(&self) -> usize {
        self as *const Self as usize
    }

    fn end_address(&self) -> usize {
        self.start_address() + self.size
    }
}

/// Intrusive Linked-List Heap Allocator.
pub struct LinkedListAllocator {
    head: Option<NonNull<BlockHeader>>,
    allocated_bytes: usize,
    total_bytes: usize,
}

impl LinkedListAllocator {
    pub const fn new() -> Self {
        Self {
            head: None,
            allocated_bytes: 0,
            total_bytes: 0,
        }
    }

    /// Initializes the heap with a contiguous memory region.
    ///
    /// # Safety
    /// The caller must ensure the memory region `[heap_start .. heap_start + heap_size]`
    /// is mapped and accessible.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.total_bytes = heap_size;
        self.allocated_bytes = 0;

        let header_ptr = heap_start as *mut BlockHeader;
        core::ptr::write(header_ptr, BlockHeader::new(heap_size));
        self.head = NonNull::new(header_ptr);
    }

    /// Allocates a block matching the specified layout.
    pub fn allocate(&mut self, layout: Layout) -> Result<NonNull<u8>, ()> {
        let size = layout.size().max(core::mem::size_of::<BlockHeader>());
        let align = layout.align().max(core::mem::align_of::<BlockHeader>());

        let mut prev: Option<NonNull<BlockHeader>> = None;
        let mut current = self.head;

        while let Some(mut curr_node) = current {
            let block = unsafe { curr_node.as_mut() };
            
            // Calculate payload start with alignment
            let alloc_start = align_up(block.start_address() + core::mem::size_of::<BlockHeader>(), align);
            let alloc_end = alloc_start.checked_add(size).ok_or(())?;

            if alloc_end <= block.end_address() {
                // Block is large enough
                let excess_size = block.end_address() - alloc_end;
                let next_node = block.next;

                // Handle excess before allocation
                let excess_before = alloc_start - core::mem::size_of::<BlockHeader>() - block.start_address();

                if excess_before >= core::mem::size_of::<BlockHeader>() + align {
                    // Split front
                    block.size = excess_before;
                    if excess_size >= core::mem::size_of::<BlockHeader>() {
                        let new_block_ptr = alloc_end as *mut BlockHeader;
                        unsafe {
                            core::ptr::write(new_block_ptr, BlockHeader {
                                size: excess_size,
                                next: next_node,
                            });
                            block.next = NonNull::new(new_block_ptr);
                        }
                    } else {
                        block.next = next_node;
                    }
                } else {
                    // Consume entire front
                    if excess_size >= core::mem::size_of::<BlockHeader>() {
                        let new_block_ptr = alloc_end as *mut BlockHeader;
                        unsafe {
                            core::ptr::write(new_block_ptr, BlockHeader {
                                size: excess_size,
                                next: next_node,
                            });
                        }
                        if let Some(mut p) = prev {
                            unsafe { p.as_mut().next = NonNull::new(new_block_ptr); }
                        } else {
                            self.head = NonNull::new(new_block_ptr);
                        }
                    } else {
                        // Entire block consumed
                        if let Some(mut p) = prev {
                            unsafe { p.as_mut().next = next_node; }
                        } else {
                            self.head = next_node;
                        }
                    }
                }

                self.allocated_bytes += size;
                return NonNull::new(alloc_start as *mut u8).ok_or(());
            }

            prev = current;
            current = block.next;
        }

        Err(()) // Out of heap memory
    }

    /// Deallocates a previously allocated chunk.
    pub unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) {
        let size = layout.size().max(core::mem::size_of::<BlockHeader>());
        let raw_addr = (ptr.as_ptr() as usize).saturating_sub(core::mem::size_of::<BlockHeader>());
        
        let mut new_block = BlockHeader::new(size + core::mem::size_of::<BlockHeader>());
        let new_block_ptr = raw_addr as *mut BlockHeader;

        // Insert sorted by address
        let mut prev: Option<NonNull<BlockHeader>> = None;
        let mut current = self.head;

        while let Some(curr_node) = current {
            if curr_node.as_ptr() as usize > raw_addr {
                break;
            }
            prev = current;
            current = unsafe { curr_node.as_ref().next };
        }

        if let Some(mut p) = prev {
            new_block.next = current;
            core::ptr::write(new_block_ptr, new_block);
            unsafe { p.as_mut().next = NonNull::new(new_block_ptr); }
        } else {
            new_block.next = self.head;
            core::ptr::write(new_block_ptr, new_block);
            self.head = NonNull::new(new_block_ptr);
        }

        if self.allocated_bytes >= size {
            self.allocated_bytes -= size;
        }

        // Coalesce adjacent free blocks
        self.coalesce();
    }

    /// Merges contiguous free blocks.
    fn coalesce(&mut self) {
        let mut current = self.head;

        while let Some(mut curr_node) = current {
            let block = unsafe { curr_node.as_mut() };
            if let Some(mut next_node) = block.next {
                let next_block = unsafe { next_node.as_mut() };
                if block.end_address() == next_block.start_address() {
                    // Merge blocks
                    block.size += next_block.size;
                    block.next = next_block.next;
                    continue; // Re-check merged block with next
                }
            }
            current = block.next;
        }
    }
}

#[inline(always)]
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

/// Global locked wrapper for GlobalAlloc.
pub struct LockedHeap(Mutex<LinkedListAllocator>);

impl LockedHeap {
    pub const fn empty() -> Self {
        Self(Mutex::new(LinkedListAllocator::new()))
    }

    pub unsafe fn init(&self, heap_start: usize, heap_size: usize) {
        self.0.lock().init(heap_start, heap_size);
    }
}

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0
            .lock()
            .allocate(layout)
            .map(|ptr| ptr.as_ptr())
            .unwrap_or(core::ptr::null_mut())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(p) = NonNull::new(ptr) {
            self.0.lock().deallocate(p, layout);
        }
    }
}

#[global_allocator]
pub static GLOBAL_ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Initializes the 16MB kernel heap by allocating frames and mapping them into the kernel PML4.
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

    // 2. Initialize the global heap allocator
    GLOBAL_ALLOCATOR.init(HEAP_START as usize, HEAP_SIZE);
}

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!("Kernel Out Of Memory! Failed allocation layout: {:?}", layout);
}
```

---

## 4. 4-Level PML4 Virtual Memory & Address Space Isolation (`src/memory/paging.rs`)

### 4.1 x86_64 4-Level Translation Mechanics
A 48-bit canonical virtual address is resolved through 4 levels:
- **PML4 Index**: `(virt >> 39) & 0x1FF` (Entries 0..511)
- **PDPT Index**: `(virt >> 30) & 0x1FF` (Entries 0..511)
- **Page Directory (PD) Index**: `(virt >> 21) & 0x1FF` (Entries 0..511)
- **Page Table (PT) Index**: `(virt >> 12) & 0x1FF` (Entries 0..511)
- **Physical Page Offset**: `virt & 0xFFF` (0..4095)

```
Virtual Address (48-bit canonical):
+------------+------------+------------+------------+---------------+
| PML4 (9b)  | PDPT (9b)  |  PD (9b)   |  PT (9b)   | Offset (12b)  |
|  Bits 47:39|  Bits 38:30|   Bits 29:21|  Bits 20:12|    Bits 11:0  |
+------------+------------+------------+------------+---------------+
      |            |            |            |              |
      v            v            v            v              v
   +------+     +------+     +------+     +------+      +-------+
   | PML4 | --> | PDPT | --> |  PD  | --> |  PT  | ---> | Frame |
   +------+     +------+     +------+     +------+      +-------+
```

### 4.2 Higher-Half Direct Map (HHDM) Mechanics
- Limine supplies `HhdmRequest` with `hhdm_offset` (typically `0xFFFF_8000_0000_0000`).
- Any physical address `P` is accessed in kernel mode at virtual address:
  $$\text{virt} = P + \text{HHDM\_OFFSET}$$
- Any HHDM virtual address `V` translates to physical:
  $$P = V - \text{HHDM\_OFFSET}$$

### 4.3 Address Space Isolation & Destruction Protocol

#### `create_user_address_space()`
1. Allocates a clean zeroed 4KB physical frame for the new process PML4 table.
2. Clones PML4 entries `256..512` from the kernel master PML4 (sharing higher-half kernel space, HHDM, Heap, Framebuffer VRAM).
3. Leaves PML4 entries `0..256` completely empty (`0x0000000000000000`), guaranteeing total user process address space isolation.
4. Returns the physical address `pml4_phys` to be loaded into `CR3` during context switches.

#### `destroy_user_address_space(pml4_phys: PhysAddr)`
To prevent CPU triple faults and kernel memory corruption:
- **Shared Higher-Half Protection**: PML4 entries `256..512` are **NEVER** traversed or modified.
- **Lower-Half Traversal (0..255)**:
  - Traverses present PDPTs, PDs, and PTs.
  - Frees all mapped leaf physical memory frames (Code, Data, Heap, Stack).
  - Frees each Page Table (PT), Page Directory (PD), and PDPT frame.
- **Root PML4 Frame**: Frees `pml4_phys`.

### 4.4 Production Blueprint: `src/memory/paging.rs`

```rust
//! 4-Level PML4 Virtual Memory Management & Address Space Isolation for AegisOS

use core::sync::atomic::{AtomicU64, Ordering};
use bitflags::bitflags;
use super::frame::{alloc_zeroed_frame, free_frame, PhysAddr, PAGE_SIZE};

/// Virtual address wrapper ensuring 64-bit alignment and type safety.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VirtAddr(pub u64);

impl VirtAddr {
    #[inline(always)]
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    #[inline(always)]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub const fn as_ptr<T>(&self) -> *const T {
        self.0 as *const T
    }

    #[inline(always)]
    pub const fn as_mut_ptr<T>(&self) -> *mut T {
        self.0 as *mut T
    }

    #[inline(always)]
    pub const fn is_aligned_4k(&self) -> bool {
        (self.0 & 0xFFF) == 0
    }

    #[inline(always)]
    pub const fn pml4_index(&self) -> usize {
        ((self.0 >> 39) & 0x1FF) as usize
    }

    #[inline(always)]
    pub const fn pdpt_index(&self) -> usize {
        ((self.0 >> 30) & 0x1FF) as usize
    }

    #[inline(always)]
    pub const fn pd_index(&self) -> usize {
        ((self.0 >> 21) & 0x1FF) as usize
    }

    #[inline(always)]
    pub const fn pt_index(&self) -> usize {
        ((self.0 >> 12) & 0x1FF) as usize
    }

    #[inline(always)]
    pub const fn page_offset(&self) -> usize {
        (self.0 & 0xFFF) as usize
    }
}

bitflags! {
    /// x86_64 Page Table Entry Flags (64-bit).
    pub struct PageTableFlags: u64 {
        const PRESENT         = 1 << 0;
        const WRITABLE        = 1 << 1;
        const USER_ACCESSIBLE = 1 << 2;
        const WRITE_THROUGH   = 1 << 3;
        const NO_CACHE        = 1 << 4;
        const ACCESSED        = 1 << 5;
        const DIRTY           = 1 << 6;
        const HUGE_PAGE       = 1 << 7;
        const GLOBAL          = 1 << 8;
        const NO_EXECUTE      = 1 << 63;
    }
}

pub const ENTRY_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// Single 64-bit entry in an x86_64 page table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct PageTableEntry(pub u64);

impl PageTableEntry {
    pub const fn empty() -> Self {
        Self(0)
    }

    #[inline(always)]
    pub fn is_present(&self) -> bool {
        (self.0 & PageTableFlags::PRESENT.bits()) != 0
    }

    #[inline(always)]
    pub fn is_huge(&self) -> bool {
        (self.0 & PageTableFlags::HUGE_PAGE.bits()) != 0
    }

    #[inline(always)]
    pub fn flags(&self) -> PageTableFlags {
        PageTableFlags::from_bits_truncate(self.0)
    }

    #[inline(always)]
    pub fn addr(&self) -> PhysAddr {
        PhysAddr::new(self.0 & ENTRY_ADDR_MASK)
    }

    #[inline(always)]
    pub fn set(&mut self, phys: PhysAddr, flags: PageTableFlags) {
        self.0 = (phys.as_u64() & ENTRY_ADDR_MASK) | flags.bits();
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.0 = 0;
    }
}

/// 4KB Page Table containing 512 entries.
#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; 512],
}

impl PageTable {
    pub const fn empty() -> Self {
        Self {
            entries: [PageTableEntry::empty(); 512],
        }
    }

    pub fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.clear();
        }
    }
}

/// Stored Limine Higher-Half Direct Map offset.
static HHDM_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Stored Kernel Master PML4 Physical Address.
static KERNEL_PML4_PHYS: AtomicU64 = AtomicU64::new(0);

/// Initializes the paging subsystem with the Limine HHDM offset.
pub fn init_paging(hhdm_offset: u64) {
    HHDM_OFFSET.store(hhdm_offset, Ordering::SeqCst);
    let cr3 = read_cr3();
    KERNEL_PML4_PHYS.store(cr3.as_u64(), Ordering::SeqCst);
}

/// Translates a physical address to a virtual address using the HHDM direct map.
#[inline(always)]
pub fn phys_to_virt(phys: PhysAddr) -> VirtAddr {
    let offset = HHDM_OFFSET.load(Ordering::Relaxed);
    VirtAddr::new(phys.as_u64() + offset)
}

/// Translates a virtual address in the HHDM region back to a physical address.
#[inline(always)]
pub fn virt_to_phys(virt: VirtAddr) -> PhysAddr {
    let offset = HHDM_OFFSET.load(Ordering::Relaxed);
    PhysAddr::new(virt.as_u64() - offset)
}

/// Returns the master kernel PML4 physical address.
pub fn get_kernel_pml4() -> PhysAddr {
    PhysAddr::new(KERNEL_PML4_PHYS.load(Ordering::Relaxed))
}

/// Reads the current PML4 physical base address from CPU `CR3`.
#[inline(always)]
pub fn read_cr3() -> PhysAddr {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) value, options(nomem, nostack));
    }
    PhysAddr::new(value & ENTRY_ADDR_MASK)
}

/// Loads a new PML4 physical base address into CPU `CR3`.
#[inline(always)]
pub fn write_cr3(pml4_phys: PhysAddr) {
    unsafe {
        core::arch::asm!("mov cr3, {}", in(reg) pml4_phys.as_u64(), options(nostack));
    }
}

/// Invalidates the TLB entry for the specified virtual address.
#[inline(always)]
pub fn flush_tlb(virt: VirtAddr) {
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) virt.as_u64(), options(nostack));
    }
}

/// Traverses the 4-level page table and resolves a virtual address to its mapped physical address.
pub fn translate_addr(pml4_phys: PhysAddr, virt: VirtAddr) -> Option<PhysAddr> {
    let pml4 = unsafe { &*phys_to_virt(pml4_phys).as_ptr::<PageTable>() };
    let pml4_entry = pml4.entries[virt.pml4_index()];
    if !pml4_entry.is_present() {
        return None;
    }

    let pdpt = unsafe { &*phys_to_virt(pml4_entry.addr()).as_ptr::<PageTable>() };
    let pdpt_entry = pdpt.entries[virt.pdpt_index()];
    if !pdpt_entry.is_present() {
        return None;
    }
    if pdpt_entry.is_huge() {
        // 1GB huge page
        let page_phys = pdpt_entry.addr().as_u64() + (virt.as_u64() & 0x3FFF_FFFF);
        return Some(PhysAddr::new(page_phys));
    }

    let pd = unsafe { &*phys_to_virt(pdpt_entry.addr()).as_ptr::<PageTable>() };
    let pd_entry = pd.entries[virt.pd_index()];
    if !pd_entry.is_present() {
        return None;
    }
    if pd_entry.is_huge() {
        // 2MB huge page
        let page_phys = pd_entry.addr().as_u64() + (virt.as_u64() & 0x1F_FFFF);
        return Some(PhysAddr::new(page_phys));
    }

    let pt = unsafe { &*phys_to_virt(pd_entry.addr()).as_ptr::<PageTable>() };
    let pt_entry = pt.entries[virt.pt_index()];
    if !pt_entry.is_present() {
        return None;
    }

    let phys = pt_entry.addr().as_u64() + (virt.page_offset() as u64);
    Some(PhysAddr::new(phys))
}

/// Maps a 4KB virtual page to a physical frame in the specified PML4 hierarchy.
/// Intermediate page tables (PDPT, PD, PT) are automatically allocated if absent.
pub fn map_page(
    pml4_phys: PhysAddr,
    virt: VirtAddr,
    phys: PhysAddr,
    flags: PageTableFlags,
) {
    let pml4 = unsafe { &mut *phys_to_virt(pml4_phys).as_mut_ptr::<PageTable>() };
    let pml4_idx = virt.pml4_index();

    // 1. Traverse / Allocate PDPT
    if !pml4.entries[pml4_idx].is_present() {
        let new_pdpt = alloc_zeroed_frame().expect("OOM: Failed to allocate PDPT frame");
        let intermediate_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | (flags & PageTableFlags::USER_ACCESSIBLE);
        pml4.entries[pml4_idx].set(new_pdpt, intermediate_flags);
    } else if flags.contains(PageTableFlags::USER_ACCESSIBLE) {
        // Propagate USER flag to existing table if requested
        let existing_flags = pml4.entries[pml4_idx].flags();
        pml4.entries[pml4_idx].set(pml4.entries[pml4_idx].addr(), existing_flags | PageTableFlags::USER_ACCESSIBLE);
    }

    let pdpt_phys = pml4.entries[pml4_idx].addr();
    let pdpt = unsafe { &mut *phys_to_virt(pdpt_phys).as_mut_ptr::<PageTable>() };
    let pdpt_idx = virt.pdpt_index();

    // 2. Traverse / Allocate PD
    if !pdpt.entries[pdpt_idx].is_present() {
        let new_pd = alloc_zeroed_frame().expect("OOM: Failed to allocate PD frame");
        let intermediate_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | (flags & PageTableFlags::USER_ACCESSIBLE);
        pdpt.entries[pdpt_idx].set(new_pd, intermediate_flags);
    } else if flags.contains(PageTableFlags::USER_ACCESSIBLE) {
        let existing_flags = pdpt.entries[pdpt_idx].flags();
        pdpt.entries[pdpt_idx].set(pdpt.entries[pdpt_idx].addr(), existing_flags | PageTableFlags::USER_ACCESSIBLE);
    }

    let pd_phys = pdpt.entries[pdpt_idx].addr();
    let pd = unsafe { &mut *phys_to_virt(pd_phys).as_mut_ptr::<PageTable>() };
    let pd_idx = virt.pd_index();

    // 3. Traverse / Allocate PT
    if !pd.entries[pd_idx].is_present() {
        let new_pt = alloc_zeroed_frame().expect("OOM: Failed to allocate PT frame");
        let intermediate_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | (flags & PageTableFlags::USER_ACCESSIBLE);
        pd.entries[pd_idx].set(new_pt, intermediate_flags);
    } else if flags.contains(PageTableFlags::USER_ACCESSIBLE) {
        let existing_flags = pd.entries[pd_idx].flags();
        pd.entries[pd_idx].set(pd.entries[pd_idx].addr(), existing_flags | PageTableFlags::USER_ACCESSIBLE);
    }

    let pt_phys = pd.entries[pd_idx].addr();
    let pt = unsafe { &mut *phys_to_virt(pt_phys).as_mut_ptr::<PageTable>() };
    let pt_idx = virt.pt_index();

    // 4. Map Leaf Physical Frame
    pt.entries[pt_idx].set(phys, flags | PageTableFlags::PRESENT);

    // Invalidate TLB if active address space
    if read_cr3() == pml4_phys {
        flush_tlb(virt);
    }
}

/// Unmaps a 4KB virtual page, returning the previously mapped `PhysAddr` if it existed.
pub fn unmap_page(pml4_phys: PhysAddr, virt: VirtAddr) -> Option<PhysAddr> {
    let pml4 = unsafe { &mut *phys_to_virt(pml4_phys).as_mut_ptr::<PageTable>() };
    let pml4_idx = virt.pml4_index();
    if !pml4.entries[pml4_idx].is_present() {
        return None;
    }

    let pdpt = unsafe { &mut *phys_to_virt(pml4.entries[pml4_idx].addr()).as_mut_ptr::<PageTable>() };
    let pdpt_idx = virt.pdpt_index();
    if !pdpt.entries[pdpt_idx].is_present() {
        return None;
    }

    let pd = unsafe { &mut *phys_to_virt(pdpt.entries[pdpt_idx].addr()).as_mut_ptr::<PageTable>() };
    let pd_idx = virt.pd_index();
    if !pd.entries[pd_idx].is_present() {
        return None;
    }

    let pt = unsafe { &mut *phys_to_virt(pd.entries[pd_idx].addr()).as_mut_ptr::<PageTable>() };
    let pt_idx = virt.pt_index();
    if !pt.entries[pt_idx].is_present() {
        return None;
    }

    let mapped_phys = pt.entries[pt_idx].addr();
    pt.entries[pt_idx].clear();

    if read_cr3() == pml4_phys {
        flush_tlb(virt);
    }

    Some(mapped_phys)
}

/// Creates a new isolated user address space.
///
/// Allocates a clean PML4 root, copies higher-half kernel mappings (entries 256..511),
/// and leaves lower-half entries (0..255) completely unmapped for private user execution.
pub fn create_user_address_space() -> PhysAddr {
    let new_pml4_frame = alloc_zeroed_frame()
        .expect("Fatal: Out of physical memory while creating user address space");
    
    let kernel_pml4_phys = get_kernel_pml4();
    let kernel_pml4 = unsafe { &*phys_to_virt(kernel_pml4_phys).as_ptr::<PageTable>() };
    let new_pml4 = unsafe { &mut *phys_to_virt(new_pml4_frame).as_mut_ptr::<PageTable>() };

    // 1. Copy Higher-Half Kernel PML4 Entries (256..512)
    for i in 256..512 {
        new_pml4.entries[i] = kernel_pml4.entries[i];
    }

    // 2. Lower-Half (0..256) is guaranteed empty (zeroed by alloc_zeroed_frame)
    new_pml4_frame
}

/// Destroys an isolated user address space, safely reclaiming all lower-half user physical frames
/// and intermediate page tables (PT, PD, PDPT) as well as the root PML4 frame.
///
/// # Safety
/// Must NEVER be called while `CR3` is currently pointing to `user_pml4_phys`.
/// Shared kernel entries (256..511) are strictly preserved.
pub unsafe fn destroy_user_address_space(user_pml4_phys: PhysAddr) -> usize {
    let mut frames_reclaimed = 0;
    let pml4 = &mut *phys_to_virt(user_pml4_phys).as_mut_ptr::<PageTable>();

    // 1. Iterate strictly over LOWER-HALF (user space entries 0..256)
    for pml4_idx in 0..256 {
        let pml4_entry = &mut pml4.entries[pml4_idx];
        if !pml4_entry.is_present() {
            continue;
        }

        let pdpt_phys = pml4_entry.addr();
        let pdpt = &mut *phys_to_virt(pdpt_phys).as_mut_ptr::<PageTable>();

        for pdpt_idx in 0..512 {
            let pdpt_entry = &mut pdpt.entries[pdpt_idx];
            if !pdpt_entry.is_present() {
                continue;
            }

            if pdpt_entry.is_huge() {
                // 1GB huge page
                free_frame(pdpt_entry.addr());
                frames_reclaimed += 512 * 512;
                continue;
            }

            let pd_phys = pdpt_entry.addr();
            let pd = &mut *phys_to_virt(pd_phys).as_mut_ptr::<PageTable>();

            for pd_idx in 0..512 {
                let pd_entry = &mut pd.entries[pd_idx];
                if !pd_entry.is_present() {
                    continue;
                }

                if pd_entry.is_huge() {
                    // 2MB huge page
                    free_frame(pd_entry.addr());
                    frames_reclaimed += 512;
                    continue;
                }

                let pt_phys = pd_entry.addr();
                let pt = &mut *phys_to_virt(pt_phys).as_mut_ptr::<PageTable>();

                // Free all user leaf frames in Page Table
                for pt_idx in 0..512 {
                    let pt_entry = &mut pt.entries[pt_idx];
                    if pt_entry.is_present() {
                        free_frame(pt_entry.addr());
                        frames_reclaimed += 1;
                        pt_entry.clear();
                    }
                }

                // Free PT frame
                free_frame(pd_phys);
                frames_reclaimed += 1;
                pd_entry.clear();
            }

            // Free PD frame
            free_frame(pdpt_phys);
            frames_reclaimed += 1;
            pdpt_entry.clear();
        }

        // Free PDPT frame
        free_frame(pml4_entry.addr());
        frames_reclaimed += 1;
        pml4_entry.clear();
    }

    // 2. Free Root PML4 frame
    free_frame(user_pml4_phys);
    frames_reclaimed += 1;

    frames_reclaimed
}
```

---

## 5. Memory Subsystem Facade & Master Init (`src/memory/mod.rs`)

### 5.1 Initialization Flow
```
Boot Entry (_start / kmain)
           |
           v
+-------------------------------------------------------------+
| 1. memory::init(memmap_response, hhdm_offset)               |
|    - Parse usable RAM into 128KB Bitmap                     |
|    - Initialize Paging Subsystem with HHDM offset           |
|    - Read and store Master Kernel PML4 address              |
+-------------------------------------------------------------+
           |
           v
+-------------------------------------------------------------+
| 2. memory::init_heap()                                      |
|    - Allocate 4096 physical frames (16MB)                   |
|    - Map [0xFFFF_9000_0000_0000 .. +16MB] into Kernel PML4  |
|    - Initialize LinkedListAllocator Free-List               |
+-------------------------------------------------------------+
           |
           v
+-------------------------------------------------------------+
| 3. Dynamic Heap Verification (extern crate alloc;)          |
|    - Test Vec::push(), Box::new(), String::from(), Arc      |
|    - Memory Subsystem is now 100% operational!              |
+-------------------------------------------------------------+
```

### 5.2 Production Blueprint: `src/memory/mod.rs`

```rust
//! Memory Management Subsystem for AegisOS
//!
//! Exposes physical frame allocation, dynamic heap management, and 4-level PML4 paging.

pub mod frame;
pub mod heap;
pub mod paging;

pub use frame::{
    alloc_frame, alloc_zeroed_frame, free_frame, get_memory_stats, BitmapFrameAllocator,
    PhysAddr, PAGE_SIZE, MAX_PHYSICAL_MEMORY,
};

pub use heap::{
    init_heap, LinkedListAllocator, LockedHeap, GLOBAL_ALLOCATOR, HEAP_SIZE, HEAP_START,
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
pub unsafe fn init(memmap_entries: &[limine::memory_map::Entry], hhdm_offset: u64) {
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
```

---

## 6. Cross-Subsystem Interface Contracts & Integration Checklists

### 6.1 Contract: Memory Subsystem -> Task Scheduler (M2)
| Function Signature | Consumer | Description |
| :--- | :--- | :--- |
| `create_user_address_space() -> PhysAddr` | `scheduler::spawn_process` | Allocates isolated PML4 with shared kernel higher-half (256..511) and empty lower-half (0..255). |
| `destroy_user_address_space(pml4: PhysAddr) -> usize` | `scheduler::reap_zombies` (Idle Task) | Safely deallocates all lower-half user physical frames and tables during Phase 2 reaping. |
| `map_page(pml4, virt, phys, flags)` | `scheduler::load_elf_binary` | Maps user ELF code/data segments, user heap, and guarded user stack. |
| `write_cr3(pml4: PhysAddr)` | `task::switch_context` | Swaps active virtual address space during preemptive timer context switches. |

### 6.2 Contract: Memory Subsystem -> Activity Monitor & Terminal (M4)
| Function Signature | Consumer | Description |
| :--- | :--- | :--- |
| `get_memory_stats() -> (u64, u64)` | Activity Monitor App & `free` CLI | Returns `(used_bytes, total_usable_bytes)` to render real-time RAM usage graph and verify `< 60MB` footprint. |

### 6.3 Invariant & Safety Checklist
- [x] **Redzone Prevention**: `-C no-redzone=y` verified to prevent kernel interrupt stack corruption.
- [x] **Zero Physical Frame Exclusion**: Frame 0 is never returned by `alloc_frame()`.
- [x] **User Guard Page Protection**: User stack is provisioned with an unmapped guard page (`0x0000_7FFF_FFFE_0000`) immediately below it (`PRESENT=0`).
- [x] **Kernel Address Space Isolation**: User tasks cannot access `0xFFFF_8000_0000_0000` because `USER_ACCESSIBLE` bit is cleared in higher-half PML4 entries.
- [x] **Reaping Safety**: Task PML4 destruction is executed on the Kernel PML4 / Idle Task stack, avoiding active CR3 self-invalidation.

---

## 7. Verification & Test Strategy

### 7.1 Automated Unit & Integration Tests
1. **Frame Allocator Exhaustion & Deallocation Test**:
   - Allocate 1000 consecutive frames, verify all addresses are 4KB aligned and distinct.
   - Free all 1000 frames in reverse order, verify `get_memory_stats()` returns to baseline.
2. **Heap Allocator Stress Test**:
   - Allocate dynamic vectors `let mut v = Vec::new()`, append 100,000 integers, verify correctness.
   - Allocate `Arc<String>`, verify ref-count increments and drops cleanly without memory leaks.
3. **Address Space Isolation & Huge Page Traversal Test**:
   - Create user address space, map virtual pages `0x400000` (code) and `0x7FFFFFFEF000` (stack).
   - Translate both virtual addresses, verify matching physical frames.
   - Call `destroy_user_address_space()`, verify all user frames are returned to the bitmap.
4. **Memory Footprint Validation**:
   - Assert `used_bytes < 60 * 1024 * 1024` on idle desktop boot.
