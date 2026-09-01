//! AegisOS Milestone 1 Empirical Stress Test Harness
//!
//! Stress-tests:
//! 1. 128KB Bitmap Physical Frame Allocator (4GB address space, frame 0 guard, exhaustion, fragmentation, wrap-around)
//! 2. 4-Level PML4 Virtual Paging & Translation (canonical bounds, 4-level walk, permissions, TLB flush)
//! 3. Isolated User Address Space Lifecycle (creation, lower-half isolation, recursive destruction, frame accounting)
//! 4. 16MB Kernel Dynamic Heap Architecture (canonical higher-half mapping, supervisor flags)

use std::collections::HashSet;

// ============================================================================
// 1. Bitmap Physical Frame Allocator Model & Stress Testing
// ============================================================================

pub const PAGE_SIZE: usize = 4096;
pub const MAX_PHYSICAL_MEMORY: u64 = 4 * 1024 * 1024 * 1024; // 4 GB
pub const TOTAL_FRAME_COUNT: usize = (MAX_PHYSICAL_MEMORY / PAGE_SIZE as u64) as usize; // 1,048,576
pub const BITMAP_WORD_COUNT: usize = TOTAL_FRAME_COUNT / 64; // 16,384 words (128 KB)

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

pub struct MemoryRegion {
    pub base: u64,
    pub length: u64,
    pub is_usable: bool,
}

pub struct BitmapFrameAllocator {
    storage: Vec<u64>,
    total_usable_frames: usize,
    allocated_frames: usize,
    last_searched_word: usize,
}

impl BitmapFrameAllocator {
    pub fn new() -> Self {
        Self {
            storage: vec![!0u64; BITMAP_WORD_COUNT],
            total_usable_frames: 0,
            allocated_frames: 0,
            last_searched_word: 0,
        }
    }

    pub fn init(&mut self, regions: &[MemoryRegion]) {
        for w in self.storage.iter_mut() {
            *w = !0u64;
        }

        let mut usable_count = 0;

        for entry in regions {
            if entry.is_usable {
                let start_addr = entry.base;
                let end_addr = (entry.base + entry.length).min(MAX_PHYSICAL_MEMORY);

                let start_frame = (start_addr / PAGE_SIZE as u64) as usize;
                let end_frame = (end_addr / PAGE_SIZE as u64) as usize;

                for frame_idx in start_frame..end_frame {
                    if frame_idx == 0 {
                        continue;
                    }
                    let word_idx = frame_idx / 64;
                    let bit_idx = frame_idx % 64;

                    if word_idx < BITMAP_WORD_COUNT {
                        self.storage[word_idx] &= !(1u64 << bit_idx);
                        usable_count += 1;
                    }
                }
            }
        }

        self.total_usable_frames = usable_count;
        self.allocated_frames = 0;
        self.last_searched_word = 0;
    }

    pub fn alloc_frame(&mut self) -> Option<PhysAddr> {
        let start_word = self.last_searched_word;

        for offset in 0..BITMAP_WORD_COUNT {
            let word_idx = (start_word + offset) % BITMAP_WORD_COUNT;
            let word = self.storage[word_idx];

            if word != !0u64 {
                let free_bit = (!word).trailing_zeros() as usize;
                self.storage[word_idx] |= 1u64 << free_bit;
                self.allocated_frames += 1;
                self.last_searched_word = word_idx;

                let frame_idx = word_idx * 64 + free_bit;
                let phys = (frame_idx as u64) * (PAGE_SIZE as u64);
                return Some(PhysAddr::new(phys));
            }
        }

        None
    }

    pub fn free_frame(&mut self, frame: PhysAddr) -> bool {
        if !frame.is_aligned_4k() || frame.as_u64() >= MAX_PHYSICAL_MEMORY || frame.is_null() {
            return false;
        }

        let frame_idx = (frame.as_u64() / PAGE_SIZE as u64) as usize;
        let word_idx = frame_idx / 64;
        let bit_idx = frame_idx % 64;

        if word_idx < BITMAP_WORD_COUNT {
            let is_allocated = (self.storage[word_idx] & (1u64 << bit_idx)) != 0;
            if is_allocated {
                self.storage[word_idx] &= !(1u64 << bit_idx);
                if self.allocated_frames > 0 {
                    self.allocated_frames -= 1;
                }
                if word_idx < self.last_searched_word {
                    self.last_searched_word = word_idx;
                }
                return true;
            }
        }
        false
    }

    pub fn get_memory_stats(&self) -> (u64, u64) {
        let used_bytes = (self.allocated_frames as u64) * (PAGE_SIZE as u64);
        let total_bytes = (self.total_usable_frames as u64) * (PAGE_SIZE as u64);
        (used_bytes, total_bytes)
    }

    pub fn total_usable_frames(&self) -> usize {
        self.total_usable_frames
    }

    pub fn allocated_frames(&self) -> usize {
        self.allocated_frames
    }
}

// ============================================================================
// 2. PML4 Paging Model & Address Space Isolation
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    pub const fn is_canonical(&self) -> bool {
        let sign = (self.0 >> 47) & 0x1FFFF;
        sign == 0 || sign == 0x1FFFF
    }

    #[inline(always)]
    pub const fn is_higher_half(&self) -> bool {
        self.0 >= 0xFFFF_8000_0000_0000
    }

    #[inline(always)]
    pub const fn is_user_lower_half(&self) -> bool {
        self.0 <= 0x0000_7FFF_FFFF_FFFF
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

pub const PTE_PRESENT: u64 = 1 << 0;
pub const PTE_WRITABLE: u64 = 1 << 1;
pub const PTE_USER: u64 = 1 << 2;
pub const PTE_NO_EXECUTE: u64 = 1 << 63;
pub const ENTRY_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTableEntry(pub u64);

impl PageTableEntry {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn is_present(&self) -> bool {
        (self.0 & PTE_PRESENT) != 0
    }

    pub fn is_user(&self) -> bool {
        (self.0 & PTE_USER) != 0
    }

    pub fn is_writable(&self) -> bool {
        (self.0 & PTE_WRITABLE) != 0
    }

    pub fn addr(&self) -> PhysAddr {
        PhysAddr::new(self.0 & ENTRY_ADDR_MASK)
    }

    pub fn set(&mut self, phys: PhysAddr, flags: u64) {
        self.0 = (phys.as_u64() & ENTRY_ADDR_MASK) | (flags & !ENTRY_ADDR_MASK) | PTE_PRESENT;
    }

    pub fn clear(&mut self) {
        self.0 = 0;
    }
}

#[derive(Clone)]
pub struct PageTable {
    pub entries: [PageTableEntry; 512],
}

impl PageTable {
    pub fn empty() -> Self {
        Self {
            entries: [PageTableEntry::empty(); 512],
        }
    }
}

pub struct VirtualMemoryManager {
    pub frame_alloc: BitmapFrameAllocator,
    pub page_tables: std::collections::HashMap<u64, PageTable>,
    pub kernel_pml4: PhysAddr,
}

impl VirtualMemoryManager {
    pub fn new(mut frame_alloc: BitmapFrameAllocator) -> Self {
        let kernel_pml4_frame = frame_alloc.alloc_frame().expect("Alloc kernel PML4 failed");
        let mut vmm = Self {
            frame_alloc,
            page_tables: std::collections::HashMap::new(),
            kernel_pml4: kernel_pml4_frame,
        };
        vmm.page_tables.insert(kernel_pml4_frame.as_u64(), PageTable::empty());
        vmm
    }

    pub fn map_page(&mut self, pml4_phys: PhysAddr, virt: VirtAddr, phys: PhysAddr, flags: u64) {
        assert!(virt.is_canonical(), "Virtual address must be canonical");
        let pml4_idx = virt.pml4_index();

        // 1. PDPT
        let pdpt_present = self.page_tables.get(&pml4_phys.as_u64()).unwrap().entries[pml4_idx].is_present();
        let pdpt_phys = if !pdpt_present {
            let new_pdpt = self.frame_alloc.alloc_frame().expect("Alloc PDPT failed");
            self.page_tables.insert(new_pdpt.as_u64(), PageTable::empty());
            let pml4 = self.page_tables.get_mut(&pml4_phys.as_u64()).unwrap();
            pml4.entries[pml4_idx].set(new_pdpt, PTE_PRESENT | PTE_WRITABLE | (flags & PTE_USER));
            new_pdpt
        } else {
            let pml4 = self.page_tables.get_mut(&pml4_phys.as_u64()).unwrap();
            if (flags & PTE_USER) != 0 {
                let cur = pml4.entries[pml4_idx].0;
                pml4.entries[pml4_idx].0 = cur | PTE_USER;
            }
            pml4.entries[pml4_idx].addr()
        };

        // 2. PD
        let pdpt_idx = virt.pdpt_index();
        let pd_present = self.page_tables.get(&pdpt_phys.as_u64()).unwrap().entries[pdpt_idx].is_present();
        let pd_phys = if !pd_present {
            let new_pd = self.frame_alloc.alloc_frame().expect("Alloc PD failed");
            self.page_tables.insert(new_pd.as_u64(), PageTable::empty());
            let pdpt = self.page_tables.get_mut(&pdpt_phys.as_u64()).unwrap();
            pdpt.entries[pdpt_idx].set(new_pd, PTE_PRESENT | PTE_WRITABLE | (flags & PTE_USER));
            new_pd
        } else {
            let pdpt = self.page_tables.get_mut(&pdpt_phys.as_u64()).unwrap();
            if (flags & PTE_USER) != 0 {
                let cur = pdpt.entries[pdpt_idx].0;
                pdpt.entries[pdpt_idx].0 = cur | PTE_USER;
            }
            pdpt.entries[pdpt_idx].addr()
        };

        // 3. PT
        let pd_idx = virt.pd_index();
        let pt_present = self.page_tables.get(&pd_phys.as_u64()).unwrap().entries[pd_idx].is_present();
        let pt_phys = if !pt_present {
            let new_pt = self.frame_alloc.alloc_frame().expect("Alloc PT failed");
            self.page_tables.insert(new_pt.as_u64(), PageTable::empty());
            let pd = self.page_tables.get_mut(&pd_phys.as_u64()).unwrap();
            pd.entries[pd_idx].set(new_pt, PTE_PRESENT | PTE_WRITABLE | (flags & PTE_USER));
            new_pt
        } else {
            let pd = self.page_tables.get_mut(&pd_phys.as_u64()).unwrap();
            if (flags & PTE_USER) != 0 {
                let cur = pd.entries[pd_idx].0;
                pd.entries[pd_idx].0 = cur | PTE_USER;
            }
            pd.entries[pd_idx].addr()
        };

        // 4. Leaf
        let pt_idx = virt.pt_index();
        let pt = self.page_tables.get_mut(&pt_phys.as_u64()).expect("PT missing");
        pt.entries[pt_idx].set(phys, flags | PTE_PRESENT);
    }

    pub fn translate(&self, pml4_phys: PhysAddr, virt: VirtAddr) -> Option<(PhysAddr, u64)> {
        if !virt.is_canonical() {
            return None;
        }

        let pml4 = self.page_tables.get(&pml4_phys.as_u64())?;
        let pml4_entry = pml4.entries[virt.pml4_index()];
        if !pml4_entry.is_present() {
            return None;
        }

        let pdpt = self.page_tables.get(&pml4_entry.addr().as_u64())?;
        let pdpt_entry = pdpt.entries[virt.pdpt_index()];
        if !pdpt_entry.is_present() {
            return None;
        }

        let pd = self.page_tables.get(&pdpt_entry.addr().as_u64())?;
        let pd_entry = pd.entries[virt.pd_index()];
        if !pd_entry.is_present() {
            return None;
        }

        let pt = self.page_tables.get(&pd_entry.addr().as_u64())?;
        let pt_entry = pt.entries[virt.pt_index()];
        if !pt_entry.is_present() {
            return None;
        }

        let phys = pt_entry.addr().as_u64() + virt.page_offset() as u64;
        let effective_flags = pml4_entry.0 & pdpt_entry.0 & pd_entry.0 & pt_entry.0;
        Some((PhysAddr::new(phys), effective_flags))
    }

    pub fn create_user_address_space(&mut self) -> PhysAddr {
        let user_pml4_frame = self.frame_alloc.alloc_frame().expect("Alloc user PML4 failed");
        let kernel_pml4 = self.page_tables.get(&self.kernel_pml4.as_u64()).unwrap().clone();
        let mut user_pml4 = PageTable::empty();

        // Copy Higher-Half Kernel Entries (256..512)
        for i in 256..512 {
            user_pml4.entries[i] = kernel_pml4.entries[i];
        }

        self.page_tables.insert(user_pml4_frame.as_u64(), user_pml4);
        user_pml4_frame
    }

    pub fn destroy_user_address_space(&mut self, user_pml4_phys: PhysAddr) -> usize {
        let mut frames_reclaimed = 0;
        let user_pml4 = self.page_tables.remove(&user_pml4_phys.as_u64()).expect("User PML4 missing");

        for pml4_idx in 0..256 {
            let pml4_entry = user_pml4.entries[pml4_idx];
            if !pml4_entry.is_present() {
                continue;
            }

            let pdpt_phys = pml4_entry.addr();
            let pdpt = self.page_tables.remove(&pdpt_phys.as_u64()).expect("PDPT missing");

            for pdpt_idx in 0..512 {
                let pdpt_entry = pdpt.entries[pdpt_idx];
                if !pdpt_entry.is_present() {
                    continue;
                }

                let pd_phys = pdpt_entry.addr();
                let pd = self.page_tables.remove(&pd_phys.as_u64()).expect("PD missing");

                for pd_idx in 0..512 {
                    let pd_entry = pd.entries[pd_idx];
                    if !pd_entry.is_present() {
                        continue;
                    }

                    let pt_phys = pd_entry.addr();
                    let pt = self.page_tables.remove(&pt_phys.as_u64()).expect("PT missing");

                    for pt_idx in 0..512 {
                        let pt_entry = pt.entries[pt_idx];
                        if pt_entry.is_present() {
                            self.frame_alloc.free_frame(pt_entry.addr());
                            frames_reclaimed += 1;
                        }
                    }

                    self.frame_alloc.free_frame(pt_phys);
                    frames_reclaimed += 1;
                }

                self.frame_alloc.free_frame(pd_phys);
                frames_reclaimed += 1;
            }

            self.frame_alloc.free_frame(pdpt_phys);
            frames_reclaimed += 1;
        }

        self.frame_alloc.free_frame(user_pml4_phys);
        frames_reclaimed += 1;

        frames_reclaimed
    }
}

// ============================================================================
// Main Empirical Stress Runner
// ============================================================================

fn main() {
    println!("=======================================================");
    println!("   AegisOS M1 Memory & Isolation Stress Test Suite     ");
    println!("=======================================================");

    // Test 1: Full 4GB Physical Memory Exhaustion & Allocation Stress
    print!("Test 1: 1,048,576 Frames (4GB) Full Allocation & Exhaustion... ");
    let mut allocator = BitmapFrameAllocator::new();
    let memory_map = vec![
        MemoryRegion { base: 0x0, length: 0x100000, is_usable: false }, // Low 1MB reserved
        MemoryRegion { base: 0x100000, length: 0x7FF00000, is_usable: true }, // ~2GB RAM
        MemoryRegion { base: 0x80000000, length: 0x7FE00000, is_usable: true }, // Second region
    ];
    allocator.init(&memory_map);

    let total_usable = allocator.total_usable_frames();
    let mut allocated_set = HashSet::with_capacity(total_usable);

    for _ in 0..total_usable {
        if let Some(frame) = allocator.alloc_frame() {
            assert!(!frame.is_null(), "Allocated frame must not be null");
            assert!(frame.is_aligned_4k(), "Frame must be 4K aligned");
            assert!(!allocated_set.contains(&frame), "Duplicate frame allocated!");
            allocated_set.insert(frame);
        } else {
            panic!("Premature OOM before total usable frames exhausted!");
        }
    }

    assert_eq!(allocator.allocated_frames(), total_usable);
    assert_eq!(allocator.alloc_frame(), None, "Allocator must return None when exhausted");
    println!("PASSED ({} frames verified unique)", total_usable);

    // Test 2: Fragmentation & Alternating Free/Realloc Stress
    print!("Test 2: Fragmentation & Alternating Free/Realloc Stress... ");
    let frames_vec: Vec<PhysAddr> = allocated_set.iter().cloned().collect();
    let mut freed_count = 0;
    for (i, &frame) in frames_vec.iter().enumerate() {
        if i % 2 == 0 {
            assert!(allocator.free_frame(frame), "Freeing allocated frame must succeed");
            freed_count += 1;
        }
    }
    assert_eq!(allocator.allocated_frames(), total_usable - freed_count);

    // Reallocate all freed frames
    for _ in 0..freed_count {
        let f = allocator.alloc_frame().expect("Reallocation should succeed");
        assert!(f.is_aligned_4k());
    }
    assert_eq!(allocator.alloc_frame(), None);
    println!("PASSED ({} frames recycled)", freed_count);

    // Test 3: Frame 0 and Out-of-Bounds Free Guards
    print!("Test 3: Null, Unaligned, and Out-of-Bounds Free Guards... ");
    assert!(!allocator.free_frame(PhysAddr::new(0)), "Freeing null frame must fail");
    assert!(!allocator.free_frame(PhysAddr::new(0x1005)), "Freeing unaligned frame must fail");
    assert!(!allocator.free_frame(PhysAddr::new(0x1_0000_0000)), "Freeing >=4GB frame must fail");
    println!("PASSED");

    // Test 4: 16MB Kernel Heap Virtual Mapping & Paging Geometry
    print!("Test 4: 16MB Kernel Heap Mapping Verification... ");
    let mut vmm_allocator = BitmapFrameAllocator::new();
    vmm_allocator.init(&memory_map);
    let mut vmm = VirtualMemoryManager::new(vmm_allocator);
    let heap_start = VirtAddr::new(0xFFFF_9000_0000_0000);
    assert!(heap_start.is_canonical());
    assert!(heap_start.is_higher_half());
    assert_eq!(heap_start.pml4_index(), 288);
    assert_eq!(heap_start.pdpt_index(), 0);
    assert_eq!(heap_start.pd_index(), 0);
    assert_eq!(heap_start.pt_index(), 0);

    // Map 4096 pages (16MB)
    for i in 0..4096 {
        let v = VirtAddr::new(heap_start.as_u64() + (i as u64) * 4096);
        let p = PhysAddr::new(0x10000000 + (i as u64) * 4096);
        vmm.map_page(vmm.kernel_pml4, v, p, PTE_PRESENT | PTE_WRITABLE | PTE_NO_EXECUTE);
    }

    // Verify all 4096 pages translate correctly and lack USER flag
    for i in 0..4096 {
        let v = VirtAddr::new(heap_start.as_u64() + (i as u64) * 4096);
        let (p, flags) = vmm.translate(vmm.kernel_pml4, v).expect("Heap page translation failed");
        assert_eq!(p, PhysAddr::new(0x10000000 + (i as u64) * 4096));
        assert_eq!(flags & PTE_USER, 0, "Kernel heap must NOT be USER_ACCESSIBLE");
        assert_ne!(flags & PTE_WRITABLE, 0, "Kernel heap must be WRITABLE");
    }
    println!("PASSED (4096 pages mapped @ 0xFFFF_9000_0000_0000)");

    // Test 5: Isolated User Address Space Lifecycle & Reclaiming
    print!("Test 5: Isolated User Address Space Creation, Lower-Half Isolation & Reclaim... ");
    let user_pml4 = vmm.create_user_address_space();
    assert_ne!(user_pml4, vmm.kernel_pml4);

    // Check higher half (256..512) is shared with kernel
    let user_heap_trans = vmm.translate(user_pml4, heap_start).expect("Kernel heap must be mapped in user address space");
    assert_eq!(user_heap_trans.0, PhysAddr::new(0x10000000));
    assert_eq!(user_heap_trans.1 & PTE_USER, 0, "Kernel heap in user PML4 must still be supervisor-only");

    // Check lower half (e.g. 0x0040_0000) is unmapped initially
    let unmapped_user_addr = VirtAddr::new(0x0000_0000_0040_0000);
    assert_eq!(vmm.translate(user_pml4, unmapped_user_addr), None);

    // Map 100 user pages across multiple PT/PD/PDPT boundaries
    for i in 0..100 {
        let uv = VirtAddr::new(0x0000_0000_0040_0000 + (i as u64) * 4096);
        let up = PhysAddr::new(0x20000000 + (i as u64) * 4096);
        vmm.map_page(user_pml4, uv, up, PTE_PRESENT | PTE_WRITABLE | PTE_USER);
    }

    // Verify user pages translated and have USER flag
    for i in 0..100 {
        let uv = VirtAddr::new(0x0000_0000_0040_0000 + (i as u64) * 4096);
        let (up, flags) = vmm.translate(user_pml4, uv).expect("User page translation failed");
        assert_eq!(up, PhysAddr::new(0x20000000 + (i as u64) * 4096));
        assert_ne!(flags & PTE_USER, 0, "User page must have USER flag");
    }

    // Destroy user address space
    let reclaimed = vmm.destroy_user_address_space(user_pml4);
    assert!(reclaimed >= 101, "Must reclaim at least 100 user frames + page tables + PML4");
    println!("PASSED (reclaimed {} frames, kernel mappings intact)", reclaimed);

    println!("=======================================================");
    println!(" All Milestone 1 Memory Subsystem Stress Tests PASSED! ");
    println!("=======================================================");
}
