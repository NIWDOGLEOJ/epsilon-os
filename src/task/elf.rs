//! ELF64 Program Loader for AegisOS
//!
//! Turns a userspace program from a byte array hand-assembled at compile time
//! into a *file* that gets parsed, validated, and mapped into a private address
//! space. Only `ET_EXEC` (fixed-load-address) images are handled; position
//! independent executables need relocation processing and a dynamic loader,
//! neither of which exists yet.
//!
//! Segment permissions from the program headers are honoured: a segment without
//! `PF_W` is mapped read-only, and one without `PF_X` is mapped `NO_EXECUTE`.
//! The latter depends on `EFER.NXE`, which `arch::syscall::init_syscall` enables
//! during boot — the loader asserts nothing about it, but a kernel that mapped
//! NX pages with NXE clear would fault on the reserved bit.

use alloc::vec::Vec;

use crate::memory::{
    alloc_zeroed_frame, map_page, phys_to_virt, PageTableFlags, PhysAddr, VirtAddr, PAGE_SIZE,
};

// -----------------------------------------------------------------------------
// On-disk structures (ELF64, little-endian)
// -----------------------------------------------------------------------------

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;

const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 0x3E;

const PT_LOAD: u32 = 1;

const PF_X: u32 = 1;
const PF_W: u32 = 2;

const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;

/// Ceiling on how much a single program may map, so that a corrupt or hostile
/// header cannot exhaust the frame allocator.
const MAX_IMAGE_PAGES: usize = 256; // 1 MiB

/// Everything a user program is allowed to occupy.
const USER_SPACE_END: u64 = 0x0000_8000_0000_0000;

/// Top of the user stack, matching `Scheduler::spawn_user_bytecode`. A loaded
/// image must not collide with it.
pub const USER_STACK_TOP: u64 = 0x0000_7FFF_FFFF_0000;

/// Stack pages given to a loaded program.
///
/// One page is enough for a payload that only faults, and nowhere near enough
/// for a real program: the Ring 3 terminal's own state is several KiB of fixed
/// buffers, and a single page put its first frame straight through the bottom
/// of the stack into unmapped memory. There is no guard page yet, so an
/// overflow still faults -- it just takes a realistic amount of recursion to
/// get there.
pub const USER_STACK_PAGES: usize = 16;

/// Lowest address the stack occupies. Segments must stay clear of it.
pub const USER_STACK_BOTTOM: u64 = USER_STACK_TOP - (USER_STACK_PAGES * PAGE_SIZE) as u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    TooSmall,
    BadMagic,
    NotElf64,
    NotLittleEndian,
    NotExecutable,
    WrongArchitecture,
    BadProgramHeaders,
    /// A `PT_LOAD` segment fell outside user space or overlapped the stack.
    SegmentOutOfRange,
    /// `filesz` exceeded `memsz`, or a segment ran past the end of the image.
    SegmentMalformed,
    /// The image asked for more pages than `MAX_IMAGE_PAGES`.
    ImageTooLarge,
    /// The entry point was not inside any loaded segment.
    EntryNotMapped,
    OutOfMemory,
}

impl ElfError {
    pub fn as_str(&self) -> &'static str {
        match self {
            ElfError::TooSmall => "image smaller than an ELF64 header",
            ElfError::BadMagic => "missing \\x7fELF magic",
            ElfError::NotElf64 => "not ELFCLASS64",
            ElfError::NotLittleEndian => "not ELFDATA2LSB",
            ElfError::NotExecutable => "not ET_EXEC",
            ElfError::WrongArchitecture => "not EM_X86_64",
            ElfError::BadProgramHeaders => "program header table out of bounds",
            ElfError::SegmentOutOfRange => "segment outside user address space",
            ElfError::SegmentMalformed => "segment filesz/offset inconsistent",
            ElfError::ImageTooLarge => "image exceeds the per-process page budget",
            ElfError::EntryNotMapped => "entry point not inside a PT_LOAD segment",
            ElfError::OutOfMemory => "frame allocator exhausted",
        }
    }
}

#[inline]
fn read_u16(bytes: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([bytes[off], bytes[off + 1]])
}

#[inline]
fn read_u32(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
}

#[inline]
fn read_u64(bytes: &[u8], off: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[off..off + 8]);
    u64::from_le_bytes(buf)
}

/// One `PT_LOAD` entry, already validated.
#[derive(Debug, Clone, Copy)]
struct LoadSegment {
    offset: usize,
    vaddr: u64,
    filesz: usize,
    memsz: usize,
    flags: u32,
}

/// Result of loading an image into an address space.
pub struct LoadedImage {
    pub entry: VirtAddr,
    /// Frames backing the image, for the PCB so the reaper can return them.
    pub frames: Vec<PhysAddr>,
}

/// Parses and validates an ELF64 header plus its `PT_LOAD` segments without
/// touching memory or allocating frames.
///
/// Split out from [`load_elf`] so the checks can be exercised on their own.
fn parse(image: &[u8]) -> Result<(u64, Vec<LoadSegment>), ElfError> {
    if image.len() < EHDR_SIZE {
        return Err(ElfError::TooSmall);
    }
    if image[0..4] != ELF_MAGIC {
        return Err(ElfError::BadMagic);
    }
    if image[4] != ELFCLASS64 {
        return Err(ElfError::NotElf64);
    }
    if image[5] != ELFDATA2LSB {
        return Err(ElfError::NotLittleEndian);
    }
    if read_u16(image, 16) != ET_EXEC {
        return Err(ElfError::NotExecutable);
    }
    if read_u16(image, 18) != EM_X86_64 {
        return Err(ElfError::WrongArchitecture);
    }

    let entry = read_u64(image, 24);
    let phoff = read_u64(image, 32) as usize;
    let phentsize = read_u16(image, 54) as usize;
    let phnum = read_u16(image, 56) as usize;

    if phentsize < PHDR_SIZE {
        return Err(ElfError::BadProgramHeaders);
    }
    let ph_table_end = phoff
        .checked_add(phnum.checked_mul(phentsize).ok_or(ElfError::BadProgramHeaders)?)
        .ok_or(ElfError::BadProgramHeaders)?;
    if ph_table_end > image.len() {
        return Err(ElfError::BadProgramHeaders);
    }

    let mut segments = Vec::new();
    let mut total_pages = 0usize;

    for i in 0..phnum {
        let base = phoff + i * phentsize;
        if read_u32(image, base) != PT_LOAD {
            continue;
        }

        let flags = read_u32(image, base + 4);
        let offset = read_u64(image, base + 8) as usize;
        let vaddr = read_u64(image, base + 16);
        let filesz = read_u64(image, base + 32) as usize;
        let memsz = read_u64(image, base + 40) as usize;

        if filesz > memsz {
            return Err(ElfError::SegmentMalformed);
        }
        match offset.checked_add(filesz) {
            Some(end) if end <= image.len() => {}
            _ => return Err(ElfError::SegmentMalformed),
        }

        // The mapped range must sit wholly inside user space and must not run
        // into the stack that `spawn_user_elf` places at the top of it.
        let seg_end = vaddr.checked_add(memsz as u64).ok_or(ElfError::SegmentOutOfRange)?;
        if seg_end > USER_SPACE_END || vaddr == 0 {
            return Err(ElfError::SegmentOutOfRange);
        }
        if seg_end > USER_STACK_BOTTOM {
            return Err(ElfError::SegmentOutOfRange);
        }

        let start_page = (vaddr as usize) & !(PAGE_SIZE - 1);
        let end_page = ((seg_end as usize) + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        total_pages += (end_page - start_page) / PAGE_SIZE;
        if total_pages > MAX_IMAGE_PAGES {
            return Err(ElfError::ImageTooLarge);
        }

        segments.push(LoadSegment { offset, vaddr, filesz, memsz, flags });
    }

    if segments.is_empty() {
        return Err(ElfError::BadProgramHeaders);
    }

    // The entry point has to land inside something we are about to map.
    let entry_mapped = segments
        .iter()
        .any(|s| entry >= s.vaddr && entry < s.vaddr + s.memsz as u64);
    if !entry_mapped {
        return Err(ElfError::EntryNotMapped);
    }

    Ok((entry, segments))
}

/// Loads an ELF64 executable into `pml4`, returning its entry point and the
/// frames it consumed.
///
/// Frames are recorded as they are allocated, so a caller that gets an `Err` can
/// still hand the partial list to the reaper rather than leaking it — see
/// `Scheduler::spawn_user_elf`.
pub fn load_elf(
    image: &[u8],
    pml4: PhysAddr,
    frames: &mut Vec<PhysAddr>,
) -> Result<VirtAddr, ElfError> {
    let (entry, segments) = parse(image)?;

    for seg in &segments {
        let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        if seg.flags & PF_W != 0 {
            flags |= PageTableFlags::WRITABLE;
        }
        if seg.flags & PF_X == 0 {
            flags |= PageTableFlags::NO_EXECUTE;
        }

        let seg_start = seg.vaddr & !(PAGE_SIZE as u64 - 1);
        let seg_end = seg.vaddr + seg.memsz as u64;
        let mut page = seg_start;

        while page < seg_end {
            // Zeroed up front, so any part of the page the file does not cover
            // (.bss, or padding either side) reads as zero without a second pass.
            let frame = alloc_zeroed_frame().ok_or(ElfError::OutOfMemory)?;
            frames.push(frame);

            // Copy whatever slice of the file belongs on this page.
            let page_end = page + PAGE_SIZE as u64;
            let file_start = core::cmp::max(page, seg.vaddr);
            let file_end = core::cmp::min(page_end, seg.vaddr + seg.filesz as u64);

            if file_end > file_start {
                let in_page = (file_start - page) as usize;
                let in_file = seg.offset + (file_start - seg.vaddr) as usize;
                let count = (file_end - file_start) as usize;

                unsafe {
                    core::ptr::copy_nonoverlapping(
                        image.as_ptr().add(in_file),
                        phys_to_virt(frame).as_mut_ptr::<u8>().add(in_page),
                        count,
                    );
                }
            }

            map_page(pml4, VirtAddr::new(page), frame, flags);
            page = page_end;
        }
    }

    Ok(VirtAddr::new(entry))
}

// -----------------------------------------------------------------------------
// Test image builder
// -----------------------------------------------------------------------------

/// Wraps raw machine code in a minimal single-segment ELF64 executable.
///
/// Exists so the loader can be exercised from `selftest` without a userspace
/// toolchain or a filesystem to read from. The layout is the simplest thing the
/// parser accepts: one ELF header, one program header, then the code, all inside
/// one `PT_LOAD` segment mapped read-execute at `vaddr`.
pub fn build_test_image(code: &[u8], vaddr: u64) -> Vec<u8> {
    let mut image = Vec::new();
    let code_offset = (EHDR_SIZE + PHDR_SIZE) as u64;

    // --- ELF header ---
    image.extend_from_slice(&ELF_MAGIC);
    image.push(ELFCLASS64);
    image.push(ELFDATA2LSB);
    image.push(1); // EI_VERSION
    image.push(0); // EI_OSABI: System V
    image.extend_from_slice(&[0u8; 8]); // EI_ABIVERSION + padding
    image.extend_from_slice(&ET_EXEC.to_le_bytes());
    image.extend_from_slice(&EM_X86_64.to_le_bytes());
    image.extend_from_slice(&1u32.to_le_bytes()); // e_version
    image.extend_from_slice(&(vaddr + code_offset).to_le_bytes()); // e_entry
    image.extend_from_slice(&(EHDR_SIZE as u64).to_le_bytes()); // e_phoff
    image.extend_from_slice(&0u64.to_le_bytes()); // e_shoff
    image.extend_from_slice(&0u32.to_le_bytes()); // e_flags
    image.extend_from_slice(&(EHDR_SIZE as u16).to_le_bytes()); // e_ehsize
    image.extend_from_slice(&(PHDR_SIZE as u16).to_le_bytes()); // e_phentsize
    image.extend_from_slice(&1u16.to_le_bytes()); // e_phnum
    image.extend_from_slice(&0u16.to_le_bytes()); // e_shentsize
    image.extend_from_slice(&0u16.to_le_bytes()); // e_shnum
    image.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx

    // --- Program header: one read-execute PT_LOAD covering the whole image ---
    let total = code_offset + code.len() as u64;
    image.extend_from_slice(&PT_LOAD.to_le_bytes());
    image.extend_from_slice(&(PF_X | 4).to_le_bytes()); // PF_X | PF_R
    image.extend_from_slice(&0u64.to_le_bytes()); // p_offset
    image.extend_from_slice(&vaddr.to_le_bytes()); // p_vaddr
    image.extend_from_slice(&vaddr.to_le_bytes()); // p_paddr
    image.extend_from_slice(&total.to_le_bytes()); // p_filesz
    image.extend_from_slice(&total.to_le_bytes()); // p_memsz
    image.extend_from_slice(&(PAGE_SIZE as u64).to_le_bytes()); // p_align

    debug_assert_eq!(image.len(), EHDR_SIZE + PHDR_SIZE);
    image.extend_from_slice(code);
    image
}

/// Parse-only entry point for tests, so header validation can be checked without
/// allocating frames or mutating an address space.
pub fn validate(image: &[u8]) -> Result<u64, ElfError> {
    parse(image).map(|(entry, _)| entry)
}
