//! `SYSCALL`/`SYSRET` Fast System Call Interface for AegisOS
//!
//! Gives Ring 3 code a way to ask the kernel for something. Before this existed a
//! user process could compute and it could fault, and nothing else — which is why
//! the only userspace in the system was a set of payloads that crash on purpose.
//!
//! # Calling convention
//!
//! Modelled on the System V / Linux x86_64 convention so that a future ELF
//! userspace can use ordinary toolchain output:
//!
//! | Register | Meaning |
//! |---|---|
//! | `rax` | syscall number (in), return value (out) |
//! | `rdi`, `rsi`, `rdx`, `r10`, `r8` | arguments 1..5 |
//! | `rcx`, `r11` | clobbered by the `syscall` instruction itself |
//!
//! Errors come back as negative values (see [`SysError`]); anything `>= 0` is a
//! successful result.
//!
//! # Two deliberate limitations
//!
//! 1. **Single CPU.** The entry stub parks the user stack pointer in a static and
//!    loads the kernel stack from another, using RIP-relative addressing rather
//!    than `swapgs` and a per-CPU block. That is correct only while exactly one
//!    core can be inside the stub at a time. This kernel has no SMP support, so
//!    that holds today; adding SMP means moving these two statics into a per-CPU
//!    structure reached through `IA32_KERNEL_GS_BASE` and pairing `swapgs` on
//!    entry and exit.
//!
//! 2. **Syscalls are not preemptible.** `IA32_FMASK` clears `IF` on entry, so a
//!    handler runs to completion with interrupts masked. This sidesteps the
//!    lock-ordering hazard described in `PROJECT.md` (a handler cannot be
//!    interrupted while holding a lock an ISR also takes), at the cost of adding
//!    handler runtime to interrupt latency. Keep handlers short.

use core::arch::global_asm;

use crate::memory::{PageTableFlags, PhysAddr, VirtAddr};

// -----------------------------------------------------------------------------
// Model-Specific Registers
// -----------------------------------------------------------------------------

const IA32_EFER: u32 = 0xC000_0080;
const IA32_STAR: u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_FMASK: u32 = 0xC000_0084;

/// `EFER.SCE` — System Call Extensions. Without this, `syscall` raises #UD.
const EFER_SCE: u64 = 1 << 0;
/// `EFER.NXE` — No-Execute Enable. Required before `PageTableFlags::NO_EXECUTE`
/// means anything; with NXE clear, bit 63 of a page table entry is reserved and
/// setting it faults. The ELF loader uses it to keep non-executable segments
/// non-executable.
const EFER_NXE: u64 = 1 << 11;

/// RFLAGS bits cleared on syscall entry: TF, IF, DF, NT and AC.
const FMASK_BITS: u64 = (1 << 8) | (1 << 9) | (1 << 10) | (1 << 14) | (1 << 18);

#[inline]
unsafe fn rdmsr(msr: u32) -> u64 {
    let (high, low): (u32, u32);
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
        options(nomem, nostack, preserves_flags)
    );
    ((high as u64) << 32) | (low as u64)
}

#[inline]
unsafe fn wrmsr(msr: u32, value: u64) {
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") (value as u32),
        in("edx") ((value >> 32) as u32),
        options(nomem, nostack, preserves_flags)
    );
}

// -----------------------------------------------------------------------------
// Per-task stack handoff (single-CPU; see the module note above)
// -----------------------------------------------------------------------------

/// Kernel stack top for the task that is currently on the CPU. Kept in step with
/// the TSS by `gdt::set_tss_rsp0`, which the scheduler already calls on every
/// context switch.
#[no_mangle]
pub static mut SYSCALL_KERNEL_RSP: u64 = 0;

/// Scratch slot holding the user `rsp` for the duration of one syscall.
#[no_mangle]
pub static mut SYSCALL_USER_RSP: u64 = 0;

// -----------------------------------------------------------------------------
// Entry stub
// -----------------------------------------------------------------------------
//
// On entry from `syscall`: rcx = user RIP, r11 = user RFLAGS, rsp = *user* stack,
// and IF is already clear courtesy of FMASK. The stack switch must therefore
// happen before anything is pushed.
//
// The argument shuffle rewrites the user convention (rax, rdi, rsi, rdx, r10, r8)
// into the SysV C convention (rdi, rsi, rdx, rcx, r8, r9) that `syscall_dispatch`
// expects. It runs highest-register-first so that no move clobbers a value a
// later move still needs.

global_asm!(
    r#"
    .global syscall_entry
    syscall_entry:
        mov [rip + SYSCALL_USER_RSP], rsp
        mov rsp, [rip + SYSCALL_KERNEL_RSP]

        push rcx                /* user RIP, for sysretq */
        push r11                /* user RFLAGS, for sysretq */
        push rbx
        push rbp
        push r12
        push r13
        push r14
        push r15

        mov r9, r8              /* arg5 */
        mov r8, r10             /* arg4 */
        mov rcx, rdx            /* arg3 */
        mov rdx, rsi            /* arg2 */
        mov rsi, rdi            /* arg1 */
        mov rdi, rax            /* syscall number */
        call syscall_dispatch
        /* return value already in rax */

        pop r15
        pop r14
        pop r13
        pop r12
        pop rbp
        pop rbx
        pop r11
        pop rcx

        mov rsp, [rip + SYSCALL_USER_RSP]
        sysretq
    "#
);

extern "C" {
    fn syscall_entry();
}

/// Enables `syscall`/`sysret` and programs the MSRs that drive it.
///
/// `STAR` encodes segment selectors by *base*, not directly: `syscall` loads
/// `CS = STAR[47:32]` and `SS = STAR[47:32] + 8`, while `sysretq` loads
/// `CS = STAR[63:48] + 16` and `SS = STAR[63:48] + 8`, forcing RPL 3. The GDT in
/// `gdt.rs` is already laid out to satisfy both (kernel code 0x08, kernel data
/// 0x10, user data 0x18, user code 0x20), so the bases are 0x08 and 0x10.
pub fn init_syscall() {
    unsafe {
        wrmsr(IA32_EFER, rdmsr(IA32_EFER) | EFER_SCE | EFER_NXE);
        wrmsr(IA32_STAR, (0x10u64 << 48) | (0x08u64 << 32));
        wrmsr(IA32_LSTAR, syscall_entry as *const () as u64);
        wrmsr(IA32_FMASK, FMASK_BITS);
    }

    crate::serial_println!(
        "[SYSCALL] SYSCALL/SYSRET enabled (EFER.SCE + EFER.NXE), entry at 0x{:016x}.",
        syscall_entry as *const () as u64
    );
}

// -----------------------------------------------------------------------------
// Syscall numbers and errors
// -----------------------------------------------------------------------------

pub const SYS_EXIT: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_YIELD: u64 = 2;
pub const SYS_GET_PID: u64 = 3;
pub const SYS_UPTIME: u64 = 4;

/// Negative return codes. Chosen to be recognisable rather than to match errno.
#[repr(i64)]
pub enum SysError {
    /// Unknown syscall number.
    NoSys = -1,
    /// A pointer argument was not a readable user mapping.
    Fault = -2,
    /// An argument was outside its permitted range.
    Invalid = -3,
}

/// Largest buffer a single `write` will accept, to bound time spent with
/// interrupts masked.
const MAX_WRITE_LEN: usize = 4096;

// -----------------------------------------------------------------------------
// User pointer validation
// -----------------------------------------------------------------------------

/// Upper bound of the 48-bit lower half. Everything the kernel owns — its own
/// image and the HHDM window onto physical memory — lives above this, so a range
/// check against it is the first line of defence.
const USER_SPACE_END: u64 = 0x0000_8000_0000_0000;

/// Translates one user virtual address, requiring `USER_ACCESSIBLE` at *every*
/// level of the walk.
///
/// The level-by-level check is the part that matters. A lower-half bound check
/// alone would already exclude the kernel's own mappings in this layout, but
/// checking the U bit at each level is what makes the guarantee independent of
/// where the kernel happens to place things, and it is what stops a user process
/// from naming a supervisor page that its own address space maps.
fn translate_user(pml4_phys: PhysAddr, virt: VirtAddr) -> Option<PhysAddr> {
    use crate::memory::{phys_to_virt, PageTable};

    if virt.as_u64() >= USER_SPACE_END {
        return None;
    }

    let user = PageTableFlags::USER_ACCESSIBLE;

    let pml4 = unsafe { &*phys_to_virt(pml4_phys).as_ptr::<PageTable>() };
    let e1 = pml4.entries[virt.pml4_index()];
    if !e1.is_present() || !e1.flags().contains(user) {
        return None;
    }

    let pdpt = unsafe { &*phys_to_virt(e1.addr()).as_ptr::<PageTable>() };
    let e2 = pdpt.entries[virt.pdpt_index()];
    if !e2.is_present() || !e2.flags().contains(user) {
        return None;
    }
    if e2.is_huge() {
        return Some(PhysAddr::new(e2.addr().as_u64() + (virt.as_u64() & 0x3FFF_FFFF)));
    }

    let pd = unsafe { &*phys_to_virt(e2.addr()).as_ptr::<PageTable>() };
    let e3 = pd.entries[virt.pd_index()];
    if !e3.is_present() || !e3.flags().contains(user) {
        return None;
    }
    if e3.is_huge() {
        return Some(PhysAddr::new(e3.addr().as_u64() + (virt.as_u64() & 0x1F_FFFF)));
    }

    let pt = unsafe { &*phys_to_virt(e3.addr()).as_ptr::<PageTable>() };
    let e4 = pt.entries[virt.pt_index()];
    if !e4.is_present() || !e4.flags().contains(user) {
        return None;
    }

    Some(PhysAddr::new(e4.addr().as_u64() + virt.page_offset() as u64))
}

/// Copies `len` bytes out of the current process's address space into `dest`.
///
/// Walks page by page, because a user buffer is only guaranteed to be contiguous
/// in *virtual* memory. Returns `false` and copies nothing further the moment any
/// page in the range fails validation.
fn copy_from_user(dest: &mut [u8], user_ptr: u64, len: usize) -> bool {
    use crate::memory::{phys_to_virt, read_cr3, PAGE_SIZE};

    if len == 0 {
        return true;
    }
    // Reject a range that leaves user space or wraps around the address space.
    match user_ptr.checked_add(len as u64) {
        Some(end) if end <= USER_SPACE_END => {}
        _ => return false,
    }

    let pml4 = read_cr3();
    let mut copied = 0usize;

    while copied < len {
        let va = user_ptr + copied as u64;
        let phys = match translate_user(pml4, VirtAddr::new(va)) {
            Some(p) => p,
            None => return false,
        };

        let page_offset = (va as usize) % PAGE_SIZE;
        let chunk = core::cmp::min(PAGE_SIZE - page_offset, len - copied);

        unsafe {
            core::ptr::copy_nonoverlapping(
                phys_to_virt(phys).as_ptr::<u8>(),
                dest.as_mut_ptr().add(copied),
                chunk,
            );
        }
        copied += chunk;
    }

    true
}

// -----------------------------------------------------------------------------
// Dispatch
// -----------------------------------------------------------------------------

/// Rust side of the syscall entry stub.
///
/// Runs with interrupts masked (see the module note), on the current task's
/// kernel stack, with the user address space still installed in CR3.
#[no_mangle]
pub extern "C" fn syscall_dispatch(num: u64, a1: u64, a2: u64, a3: u64, _a4: u64, _a5: u64) -> u64 {
    match num {
        SYS_EXIT => sys_exit(a1 as i64),
        SYS_WRITE => sys_write(a1, a2, a3 as usize),
        SYS_YIELD => sys_yield(),
        SYS_GET_PID => crate::task::current_pid() as u64,
        SYS_UPTIME => crate::task::get_uptime_ticks(),
        _ => SysError::NoSys as i64 as u64,
    }
}

/// Terminates the calling process. Does not return.
fn sys_exit(code: i64) -> u64 {
    use crate::task::pcb::{ExitReason, TaskState};
    use crate::task::scheduler::SCHEDULER;

    {
        let mut sched = SCHEDULER.lock();
        let idx = sched.current_idx;
        if let Some(pcb) = sched.tasks.get_mut(idx) {
            let pid = pcb.pid;
            crate::arch::serial::_print(format_args!(
                "[SYSCALL] Process PID {} ('{}') exited with code {}.\n",
                pid, pcb.name, code
            ));
            pcb.state = TaskState::Terminated(ExitReason::Normal(code as i32));
            if !sched.zombie_queue.contains(&pid) {
                sched.zombie_queue.push(pid);
            }
        }
    }

    // The task is now Terminated, so `Scheduler::schedule` will step over it and
    // never select it again. Releasing interrupts and halting hands the CPU to
    // the next timer tick, which switches away and does not come back. Returning
    // to `sysretq` instead would resume a process that no longer exists.
    loop {
        unsafe {
            core::arch::asm!("sti; hlt", options(nomem, nostack));
        }
    }
}

/// Writes a user buffer to the serial console. `fd` 1 (stdout) and 2 (stderr)
/// are accepted; there is no file descriptor table yet.
fn sys_write(fd: u64, buf: u64, len: usize) -> u64 {
    if fd != 1 && fd != 2 {
        return SysError::Invalid as i64 as u64;
    }
    if len > MAX_WRITE_LEN {
        return SysError::Invalid as i64 as u64;
    }

    let mut scratch = [0u8; MAX_WRITE_LEN];
    if !copy_from_user(&mut scratch[..len], buf, len) {
        return SysError::Fault as i64 as u64;
    }

    // Anything that is not printable ASCII is rendered as '.', so that a wild
    // pointer cannot drive the terminal through escape sequences.
    for &byte in &scratch[..len] {
        let c = match byte {
            b'\n' | b'\t' => byte,
            0x20..=0x7E => byte,
            _ => b'.',
        };
        crate::arch::serial::_print(format_args!("{}", c as char));
    }

    len as u64
}

/// Yields the rest of the current time slice.
fn sys_yield() -> u64 {
    use crate::task::scheduler::SCHEDULER;
    {
        let mut sched = SCHEDULER.lock();
        let idx = sched.current_idx;
        if let Some(pcb) = sched.tasks.get_mut(idx) {
            pcb.time_slice_remaining = 0;
        }
    }
    // The next timer tick sees an exhausted quantum and rotates. `sti; hlt` waits
    // for it; `IF` is restored to the user's saved RFLAGS by `sysretq` regardless.
    unsafe {
        core::arch::asm!("sti; hlt; cli", options(nomem, nostack));
    }
    0
}
