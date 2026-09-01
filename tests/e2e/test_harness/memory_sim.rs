//! AegisOS E2E Test Harness: Physical Bitmap Frame & PML4 Paging Simulator
//!
//! Models 4GB RAM bitmap frame allocation and 4-level x86_64 paging with
//! hardware privilege protection and fault detection.

use super::types::*;
use std::collections::HashMap;

pub struct FrameAllocSimulator {
    bitmap: Vec<u8>,
    total_frames: usize,
    allocated_count: usize,
}

impl FrameAllocSimulator {
    pub fn new(total_ram_bytes: u64) -> Self {
        let total_frames = (total_ram_bytes / PAGE_SIZE as u64) as usize;
        let bitmap_bytes = (total_frames + 7) / 8;
        Self {
            bitmap: vec![0u8; bitmap_bytes],
            total_frames,
            allocated_count: 0,
        }
    }

    pub fn new_4gb() -> Self {
        Self::new(TOTAL_RAM_4GB)
    }

    pub fn alloc_frame(&mut self) -> Option<PhysAddr> {
        for frame_idx in 0..self.total_frames {
            let byte_idx = frame_idx / 8;
            let bit_idx = frame_idx % 8;
            if (self.bitmap[byte_idx] & (1 << bit_idx)) == 0 {
                self.bitmap[byte_idx] |= 1 << bit_idx;
                self.allocated_count += 1;
                return Some(PhysAddr((frame_idx * PAGE_SIZE) as u64));
            }
        }
        None
    }

    pub fn free_frame(&mut self, frame: PhysAddr) -> bool {
        if !frame.is_aligned_4k() {
            return false;
        }
        let frame_idx = (frame.as_u64() / PAGE_SIZE as u64) as usize;
        if frame_idx >= self.total_frames {
            return false;
        }
        let byte_idx = frame_idx / 8;
        let bit_idx = frame_idx % 8;
        if (self.bitmap[byte_idx] & (1 << bit_idx)) != 0 {
            self.bitmap[byte_idx] &= !(1 << bit_idx);
            self.allocated_count = self.allocated_count.saturating_sub(1);
            true
        } else {
            false // Double free or freeing unallocated frame
        }
    }

    pub fn alloc_contiguous(&mut self, count: usize) -> Option<PhysAddr> {
        if count == 0 || count > self.total_frames {
            return None;
        }
        let mut consecutive = 0;
        let mut start_idx = 0;

        for frame_idx in 0..self.total_frames {
            let byte_idx = frame_idx / 8;
            let bit_idx = frame_idx % 8;
            if (self.bitmap[byte_idx] & (1 << bit_idx)) == 0 {
                if consecutive == 0 {
                    start_idx = frame_idx;
                }
                consecutive += 1;
                if consecutive == count {
                    for i in start_idx..(start_idx + count) {
                        let b = i / 8;
                        let bi = i % 8;
                        self.bitmap[b] |= 1 << bi;
                    }
                    self.allocated_count += count;
                    return Some(PhysAddr((start_idx * PAGE_SIZE) as u64));
                }
            } else {
                consecutive = 0;
            }
        }
        None
    }

    pub fn allocated_count(&self) -> usize {
        self.allocated_count
    }

    pub fn free_count(&self) -> usize {
        self.total_frames.saturating_sub(self.allocated_count)
    }

    pub fn total_frames(&self) -> usize {
        self.total_frames
    }

    pub fn used_bytes(&self) -> u64 {
        (self.allocated_count as u64) * PAGE_SIZE as u64
    }

    pub fn is_frame_allocated(&self, frame: PhysAddr) -> bool {
        let frame_idx = (frame.as_u64() / PAGE_SIZE as u64) as usize;
        if frame_idx >= self.total_frames {
            return false;
        }
        (self.bitmap[frame_idx / 8] & (1 << (frame_idx % 8))) != 0
    }
}

#[derive(Debug, Clone)]
pub struct PageMapping {
    pub phys_addr: PhysAddr,
    pub flags: u64,
}

#[derive(Debug, Clone)]
pub struct Pml4Simulator {
    pub pml4_phys: PhysAddr,
    mappings: HashMap<u64 /* virt page aligned */, PageMapping>,
}

impl Pml4Simulator {
    pub fn new(pml4_phys: PhysAddr) -> Self {
        let mut sim = Self {
            pml4_phys,
            mappings: HashMap::new(),
        };
        // Seed Higher-Half Direct Map (HHDM) kernel space
        sim.init_kernel_higher_half();
        sim
    }

    fn init_kernel_higher_half(&mut self) {
        // Map 0xFFFF_8000_0000_0000.. (HHDM) and 0xFFFF_FFFF_8000_0000.. (Kernel text/data)
        // With Ring 0 Supervisor privileges only (PTE_PRESENT | PTE_WRITABLE, no PTE_USER)
        let hhdm_page = HHDM_OFFSET & !0xFFF;
        self.mappings.insert(
            hhdm_page,
            PageMapping {
                phys_addr: PhysAddr(0x0),
                flags: PTE_PRESENT | PTE_WRITABLE, // Ring 0 only
            },
        );

        let kernel_page = KERNEL_VIRTUAL_BASE & !0xFFF;
        self.mappings.insert(
            kernel_page,
            PageMapping {
                phys_addr: PhysAddr(0x100000), // Kernel physical load addr
                flags: PTE_PRESENT | PTE_WRITABLE, // Ring 0 only
            },
        );
    }

    pub fn map_page(&mut self, virt: VirtAddr, phys: PhysAddr, flags: u64) -> Result<(), &'static str> {
        if !virt.is_canonical() {
            return Err("Non-canonical virtual address");
        }
        if !virt.as_u64().is_aligned_4k() || !phys.as_u64().is_aligned_4k() {
            return Err("Unaligned page address");
        }
        let page_key = virt.as_u64() & !0xFFF;
        self.mappings.insert(
            page_key,
            PageMapping {
                phys_addr: phys,
                flags: flags | PTE_PRESENT,
            },
        );
        Ok(())
    }

    pub fn unmap_page(&mut self, virt: VirtAddr) -> Option<PhysAddr> {
        let page_key = virt.as_u64() & !0xFFF;
        self.mappings.remove(&page_key).map(|m| m.phys_addr)
    }

    pub fn translate(&self, virt: VirtAddr, is_user: bool, is_write: bool) -> Result<PhysAddr, PageFaultErrorCode> {
        if !virt.is_canonical() {
            return Err(PageFaultErrorCode {
                present: false,
                write: is_write,
                user: is_user,
                reserved_write: false,
                instruction_fetch: false,
            });
        }

        let page_key = virt.as_u64() & !0xFFF;
        let offset = virt.page_offset();

        match self.mappings.get(&page_key) {
            None => {
                // Not present fault
                Err(PageFaultErrorCode {
                    present: false,
                    write: is_write,
                    user: is_user,
                    reserved_write: false,
                    instruction_fetch: false,
                })
            }
            Some(mapping) => {
                // Check Present bit
                if (mapping.flags & PTE_PRESENT) == 0 {
                    return Err(PageFaultErrorCode {
                        present: false,
                        write: is_write,
                        user: is_user,
                        reserved_write: false,
                        instruction_fetch: false,
                    });
                }

                // Check Ring 3 User vs Ring 0 Supervisor access
                if is_user && (mapping.flags & PTE_USER) == 0 {
                    // Ring 3 trying to access Ring 0 supervisor page!
                    return Err(PageFaultErrorCode {
                        present: true,
                        write: is_write,
                        user: true,
                        reserved_write: false,
                        instruction_fetch: false,
                    });
                }

                // Check Write permissions
                if is_write && (mapping.flags & PTE_WRITABLE) == 0 {
                    // Write to read-only page!
                    return Err(PageFaultErrorCode {
                        present: true,
                        write: true,
                        user: is_user,
                        reserved_write: false,
                        instruction_fetch: false,
                    });
                }

                Ok(PhysAddr(mapping.phys_addr.as_u64() + offset))
            }
        }
    }

    pub fn clone_for_user_process(&self, new_pml4_phys: PhysAddr) -> Self {
        let mut user_pml4 = Self::new(new_pml4_phys);
        // Shared higher-half mappings are preserved, lower-half user mappings start isolated/empty
        user_pml4
    }

    pub fn user_page_count(&self) -> usize {
        self.mappings
            .iter()
            .filter(|(&virt, m)| virt <= 0x0000_7FFF_FFFF_FFFF && (m.flags & PTE_USER) != 0)
            .count()
    }
}

trait UnalignedCheck {
    fn is_aligned_4k(&self) -> bool;
}

impl UnalignedCheck for u64 {
    fn is_aligned_4k(&self) -> bool {
        (self & 0xFFF) == 0
    }
}
