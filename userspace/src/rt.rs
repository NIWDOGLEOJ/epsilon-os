//! Minimal Ring 3 runtime: entry point and panic handling.
//!
//! Included directly by each binary with `#[path]` rather than living in the
//! library, so that every program gets exactly one `_start` and one
//! `#[panic_handler]` and the linker is never asked to pull them out of an
//! rlib.
//!
//! There is no libc and no crt0 beneath this. The kernel's ELF loader sets
//! `rsp` to the top of a one-page stack and jumps straight to `_start`.

use aegis_user::sys;

/// Process entry point, named to match `ENTRY(_start)` in `linker.ld`.
///
/// The stack the kernel hands over is already 16-byte aligned, but the ABI wants
/// `rsp % 16 == 8` on entry to a function (as if a return address had been
/// pushed), so this aligns and then calls rather than jumping.
#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "and rsp, -16",
        "xor rbp, rbp",
        "call main",
        // `main` is `-> !`, so this is only reached if something goes wrong.
        "ud2",
    )
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    sys::write_str("[USERSPACE PANIC] ");
    if let Some(loc) = info.location() {
        sys::write_str(loc.file());
    } else {
        sys::write_str("<unknown location>");
    }
    sys::write_str("\n");
    // A userspace panic exits the process. The kernel keeps running -- that is
    // the entire point of moving this out of Ring 0.
    sys::exit(101)
}
