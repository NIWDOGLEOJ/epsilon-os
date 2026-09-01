//! 4-Level PML4 Virtual Memory Management & Address Space Isolation for AegisOS

use core::sync::atomic::{AtomicU64, Ordering};
use bitflags::bitflags;
use super::frame::{alloc_zeroed_frame, free_frame, PhysAddr};

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
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
                free_frame(pt_phys);
                frames_reclaimed += 1;
                pd_entry.clear();
            }

            // Free PD frame
            free_frame(pd_phys);
            frames_reclaimed += 1;
            pdpt_entry.clear();
        }

        // Free PDPT frame
        free_frame(pdpt_phys);
        frames_reclaimed += 1;
        pml4_entry.clear();
    }

    // 2. Free Root PML4 frame
    free_frame(user_pml4_phys);
    frames_reclaimed += 1;

    frames_reclaimed
}
