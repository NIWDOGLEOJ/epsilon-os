//! Shared Window Surfaces for Ring 3 Applications
//!
//! A Ring 0 app draws by being handed `&mut Framebuffer` and scribbling on it.
//! A Ring 3 app cannot be handed a kernel pointer, so it gets a surface instead:
//! a block of pixels the kernel owns, mapped writable into the process's address
//! space, which the compositor blits into that window's client rect each frame.
//!
//! This is the same shape as a shared-memory surface in a real display server,
//! and it is deliberately the *only* way a user process can put pixels on screen
//! -- there is no syscall that draws, so a compromised or confused process can
//! corrupt its own window and nothing else.
//!
//! # Scope
//!
//! Up to [`MAX_SURFACES`] processes hold a surface at once, keyed by PID. Each
//! is allocated on first use and mapped at the same user address in every
//! address space, since each process has its own.

use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::InterruptGuard;
use crate::memory::{
    alloc_zeroed_frame, map_page, phys_to_virt, PageTableFlags, PhysAddr, VirtAddr, PAGE_SIZE,
};

/// Surface dimensions. Fixed rather than negotiated: the compositor clips the
/// blit to the window's client rect, so a resized window shows more or less of
/// the surface instead of requiring a realloc and a protocol to announce it.
pub const SURFACE_WIDTH: usize = 640;
pub const SURFACE_HEIGHT: usize = 384;
pub const SURFACE_BYTES: usize = SURFACE_WIDTH * SURFACE_HEIGHT * 4;

/// Where the surface appears in the user address space. Clear of the program
/// image at 0x400000 and of the stack at the top of the lower half.
pub const USER_SURFACE_BASE: u64 = 0x1000_0000;

/// How many Ring 3 GUI processes can hold a surface at once.
///
/// Small and fixed: each surface is ~960 KiB, and slots are only populated on
/// first use, so the cost is per live GUI process rather than per slot.
pub const MAX_SURFACES: usize = 4;

struct SurfaceSlot {
    /// Physical frames backing the pixels, in order. Empty until first use.
    frames: Vec<PhysAddr>,
    /// PID this surface belongs to, if any.
    owner: Option<u64>,
}

impl SurfaceSlot {
    const fn new() -> Self {
        Self { frames: Vec::new(), owner: None }
    }
}

pub struct Surfaces {
    slots: [SurfaceSlot; MAX_SURFACES],
}

impl Surfaces {
    const fn new() -> Self {
        Self { slots: [const { SurfaceSlot::new() }; MAX_SURFACES] }
    }

    fn index_of(&self, pid: u64) -> Option<usize> {
        self.slots.iter().position(|s| s.owner == Some(pid))
    }
}

pub static SURFACES: Mutex<Surfaces> = Mutex::new(Surfaces::new());

/// Number of frames a full surface occupies.
pub const SURFACE_FRAME_COUNT: usize = SURFACE_BYTES.div_ceil(PAGE_SIZE);

/// Allocates a surface for `pid` if it has none, and maps it into `pml4`.
///
/// Returns the user virtual base address, or `None` if no slot is free or
/// frames ran out. Calling twice from the same process returns the same
/// address and re-maps, which is harmless.
pub fn map_for_process(pml4: PhysAddr, pid: u64) -> Option<u64> {
    let _guard = InterruptGuard::acquire();
    let mut surfaces = SURFACES.lock();

    // Claim this process's existing slot, or the first free one. A slot whose
    // frames are already allocated is reused as-is; the pixels are stale, and
    // the new owner overwrites them on its first frame.
    let index = match surfaces.index_of(pid) {
        Some(index) => index,
        None => {
            let free = surfaces.slots.iter().position(|s| s.owner.is_none())?;
            surfaces.slots[free].owner = Some(pid);
            free
        }
    };

    if surfaces.slots[index].frames.is_empty() {
        let page_count = SURFACE_FRAME_COUNT;
        let mut frames = Vec::with_capacity(page_count);
        for _ in 0..page_count {
            match alloc_zeroed_frame() {
                Some(frame) => frames.push(frame),
                None => {
                    // Give back whatever was taken rather than stranding it.
                    for frame in frames {
                        crate::memory::free_frame(frame);
                    }
                    surfaces.slots[index].owner = None;
                    return None;
                }
            }
        }
        surfaces.slots[index].frames = frames;
    }

    // NO_EXECUTE: the process draws here, it does not run here.
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;

    for (i, &frame) in surfaces.slots[index].frames.iter().enumerate() {
        let virt = VirtAddr::new(USER_SURFACE_BASE + (i * PAGE_SIZE) as u64);
        map_page(pml4, virt, frame, flags);
    }

    Some(USER_SURFACE_BASE)
}

/// Copies `pid`'s surface frame list into `out`, returning how many were
/// written, or 0 if that process has no surface.
///
/// This exists so the compositor never holds the surface lock across a blit.
///
/// Holding it would be a lock-ordering bug of exactly the kind `PROJECT.md`
/// describes: the compositor runs in task context with interrupts enabled,
/// while `map_for_process` takes the same lock from syscall context where
/// `IA32_FMASK` has already cleared `IF`. Preempt the compositor mid-blit,
/// schedule the Ring 3 process, and its next `SYS_SURFACE_MAP` spins on a lock
/// whose holder can never be rescheduled to release it.
///
/// Copying the list under a brief guarded lock and reading the pixels without
/// one keeps the critical section to a few hundred bytes. Reading frames the
/// process is concurrently writing can tear, which is what a shared framebuffer
/// is; it cannot fault, because the frames stay allocated for the life of the
/// system.
pub fn snapshot_frames(pid: u64, out: &mut [PhysAddr; SURFACE_FRAME_COUNT]) -> usize {
    let _guard = InterruptGuard::acquire();
    let surfaces = SURFACES.lock();
    let Some(index) = surfaces.index_of(pid) else {
        return 0;
    };
    let frames = &surfaces.slots[index].frames;
    let count = frames.len().min(out.len());
    out[..count].copy_from_slice(&frames[..count]);
    count
}

/// Reads one pixel from a snapshot taken by [`snapshot_frames`].
#[inline]
pub fn pixel_from(frames: &[PhysAddr], x: usize, y: usize) -> u32 {
    if x >= SURFACE_WIDTH || y >= SURFACE_HEIGHT {
        return 0;
    }
    let byte_offset = (y * SURFACE_WIDTH + x) * 4;
    let frame_index = byte_offset / PAGE_SIZE;
    if frame_index >= frames.len() {
        return 0;
    }
    let in_frame = byte_offset % PAGE_SIZE;
    let base = phys_to_virt(frames[frame_index]).as_ptr::<u8>();
    unsafe { core::ptr::read_volatile(base.add(in_frame) as *const u32) }
}

/// Drops a dead process's claim so its slot can be reused. The frames stay
/// allocated; the next owner overwrites them.
pub fn release(pid: u64) {
    let _guard = InterruptGuard::acquire();
    let mut surfaces = SURFACES.lock();
    if let Some(index) = surfaces.index_of(pid) {
        surfaces.slots[index].owner = None;
    }
}
