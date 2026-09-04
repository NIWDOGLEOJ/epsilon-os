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
//! One surface, for one Ring 3 GUI process. The surface is allocated on first
//! use and mapped into whichever process asks for it. Supporting several would
//! mean keying these statics by PID; nothing here assumes a single process
//! except `SURFACE`'s singleton-ness, so that is a contained change.

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

pub struct Surface {
    /// Physical frames backing the pixels, in order.
    frames: Vec<PhysAddr>,
    /// PID the surface is currently mapped into, if any.
    owner: Option<u64>,
}

impl Surface {
    pub const fn new() -> Self {
        Self { frames: Vec::new(), owner: None }
    }

    pub fn is_mapped(&self) -> bool {
        !self.frames.is_empty() && self.owner.is_some()
    }

    pub fn owner(&self) -> Option<u64> {
        self.owner
    }
}

/// Number of frames a full surface occupies.
pub const SURFACE_FRAME_COUNT: usize = SURFACE_BYTES.div_ceil(PAGE_SIZE);

/// Copies the surface's frame list into `out`, returning how many were written.
///
/// This exists so the compositor never holds `SURFACE` across a blit.
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
pub fn snapshot_frames(out: &mut [PhysAddr; SURFACE_FRAME_COUNT]) -> usize {
    let _guard = InterruptGuard::acquire();
    let surface = SURFACE.lock();
    let count = surface.frames.len().min(out.len());
    out[..count].copy_from_slice(&surface.frames[..count]);
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

pub static SURFACE: Mutex<Surface> = Mutex::new(Surface::new());

/// Allocates the surface if needed and maps it into `pml4` for `pid`.
///
/// Returns the user virtual base address, or `None` if frames ran out. Calling
/// twice from the same process is harmless and returns the same address.
pub fn map_for_process(pml4: PhysAddr, pid: u64) -> Option<u64> {
    let _guard = InterruptGuard::acquire();
    let mut surface = SURFACE.lock();

    if surface.frames.is_empty() {
        let page_count = SURFACE_BYTES.div_ceil(PAGE_SIZE);
        let mut frames = Vec::with_capacity(page_count);
        for _ in 0..page_count {
            match alloc_zeroed_frame() {
                Some(frame) => frames.push(frame),
                None => {
                    // Give back whatever was taken rather than stranding it.
                    for frame in frames {
                        crate::memory::free_frame(frame);
                    }
                    return None;
                }
            }
        }
        surface.frames = frames;
    }

    // NO_EXECUTE: the process draws here, it does not run here.
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;

    for (i, &frame) in surface.frames.iter().enumerate() {
        let virt = VirtAddr::new(USER_SURFACE_BASE + (i * PAGE_SIZE) as u64);
        map_page(pml4, virt, frame, flags);
    }

    surface.owner = Some(pid);
    Some(USER_SURFACE_BASE)
}

/// Drops the surface's claim when its process dies, so the next Ring 3 GUI
/// process can take it. The frames stay allocated for reuse.
pub fn release(pid: u64) {
    let _guard = InterruptGuard::acquire();
    let mut surface = SURFACE.lock();
    if surface.owner == Some(pid) {
        surface.owner = None;
    }
}
