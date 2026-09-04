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
//! Every other register is preserved across the call, matching what Linux
//! promises and therefore what any toolchain targeting this convention will
//! assume. The entry stub saves the argument registers as well as the
//! callee-saved set, because the Rust dispatcher it calls treats the argument
//! registers as scratch.
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
        /* Move the saved user rsp onto this task's kernel stack immediately.
           The global is a two-instruction scratch slot, safe only because
           FMASK cleared IF so nothing can run in between. Leaving the value
           there for the duration of the call would be a bug: a syscall that
           re-enables interrupts can be preempted, and the next process to make
           a syscall would overwrite it -- returning this one onto a stack
           pointer belonging to somebody else. */
        push [rip + SYSCALL_USER_RSP]

        push rcx                /* user RIP, for sysretq */
        push r11                /* user RFLAGS, for sysretq */
        push rbx
        push rbp
        push r12
        push r13
        push r14
        push r15
        /* The argument registers are caller-saved in SysV, so
           `syscall_dispatch` is free to destroy them. Userspace is promised
           that only rcx and r11 are clobbered, so they are saved here too --
           before the shuffle below reads them. 14 pushes keeps rsp 16-byte
           aligned at the call. */
        push rdi
        push rsi
        push rdx
        push r10
        push r8
        push r9

        mov r9, r8              /* arg5 */
        mov r8, r10             /* arg4 */
        mov rcx, rdx            /* arg3 */
        mov rdx, rsi            /* arg2 */
        mov rsi, rdi            /* arg1 */
        mov rdi, rax            /* syscall number */
        /* 15 pushes leave rsp 8 mod 16; SysV wants 0 mod 16 at the call. */
        sub rsp, 8
        call syscall_dispatch
        add rsp, 8
        /* return value already in rax, and deliberately not restored */

        pop r9
        pop r8
        pop r10
        pop rdx
        pop rsi
        pop rdi
        pop r15
        pop r14
        pop r13
        pop r12
        pop rbp
        pop rbx
        pop r11
        pop rcx
        pop rsp                 /* user rsp, saved per-task above */
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
pub const SYS_SURFACE_MAP: u64 = 5;
pub const SYS_SURFACE_DIMS: u64 = 6;
pub const SYS_POLL_EVENT: u64 = 7;
pub const SYS_PROC_COUNT: u64 = 8;
pub const SYS_PROC_INFO: u64 = 9;
pub const SYS_MEM_STATS: u64 = 10;
pub const SYS_KILL: u64 = 11;
pub const SYS_FS_COUNT: u64 = 12;
pub const SYS_FS_NAME: u64 = 13;
pub const SYS_FS_READ: u64 = 14;
pub const SYS_BEEP: u64 = 15;

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

/// Copies `src` into the current process's address space at `user_ptr`.
///
/// The mirror of `copy_from_user`, and additionally requires `WRITABLE` at every
/// level: a process must not be able to talk the kernel into writing through a
/// read-only mapping such as its own `.text`.
fn copy_to_user(user_ptr: u64, src: &[u8]) -> bool {
    use crate::memory::{phys_to_virt, read_cr3, PAGE_SIZE};

    if src.is_empty() {
        return true;
    }
    match user_ptr.checked_add(src.len() as u64) {
        Some(end) if end <= USER_SPACE_END => {}
        _ => return false,
    }

    let pml4 = read_cr3();
    let mut copied = 0usize;

    while copied < src.len() {
        let va = user_ptr + copied as u64;
        let phys = match translate_user_writable(pml4, VirtAddr::new(va)) {
            Some(p) => p,
            None => return false,
        };

        let page_offset = (va as usize) % PAGE_SIZE;
        let chunk = core::cmp::min(PAGE_SIZE - page_offset, src.len() - copied);

        unsafe {
            core::ptr::copy_nonoverlapping(
                src.as_ptr().add(copied),
                phys_to_virt(phys).as_mut_ptr::<u8>(),
                chunk,
            );
        }
        copied += chunk;
    }

    true
}

/// As `translate_user`, but also requires `WRITABLE` at every level.
fn translate_user_writable(pml4_phys: PhysAddr, virt: VirtAddr) -> Option<PhysAddr> {
    use crate::memory::{phys_to_virt, PageTable};

    if virt.as_u64() >= USER_SPACE_END {
        return None;
    }

    let needed = PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE;

    let pml4 = unsafe { &*phys_to_virt(pml4_phys).as_ptr::<PageTable>() };
    let e1 = pml4.entries[virt.pml4_index()];
    if !e1.is_present() || !e1.flags().contains(needed) {
        return None;
    }

    let pdpt = unsafe { &*phys_to_virt(e1.addr()).as_ptr::<PageTable>() };
    let e2 = pdpt.entries[virt.pdpt_index()];
    if !e2.is_present() || !e2.flags().contains(needed) {
        return None;
    }
    if e2.is_huge() {
        return Some(PhysAddr::new(e2.addr().as_u64() + (virt.as_u64() & 0x3FFF_FFFF)));
    }

    let pd = unsafe { &*phys_to_virt(e2.addr()).as_ptr::<PageTable>() };
    let e3 = pd.entries[virt.pd_index()];
    if !e3.is_present() || !e3.flags().contains(needed) {
        return None;
    }
    if e3.is_huge() {
        return Some(PhysAddr::new(e3.addr().as_u64() + (virt.as_u64() & 0x1F_FFFF)));
    }

    let pt = unsafe { &*phys_to_virt(e3.addr()).as_ptr::<PageTable>() };
    let e4 = pt.entries[virt.pt_index()];
    if !e4.is_present() || !e4.flags().contains(needed) {
        return None;
    }

    Some(PhysAddr::new(e4.addr().as_u64() + virt.page_offset() as u64))
}

/// Reads a user string argument into a stack buffer, returning it as `&str`.
/// Rejects anything longer than the buffer or not valid UTF-8.
fn read_user_str<'a>(buf: &'a mut [u8], ptr: u64, len: usize) -> Option<&'a str> {
    if len > buf.len() {
        return None;
    }
    if !copy_from_user(&mut buf[..len], ptr, len) {
        return None;
    }
    core::str::from_utf8(&buf[..len]).ok()
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
        SYS_SURFACE_MAP => sys_surface_map(),
        SYS_SURFACE_DIMS => {
            ((crate::gui::surface::SURFACE_WIDTH as u64) << 32)
                | crate::gui::surface::SURFACE_HEIGHT as u64
        }
        SYS_POLL_EVENT => crate::task::uevent::poll(crate::task::current_pid()),
        SYS_PROC_COUNT => crate::task::get_process_list().len() as u64,
        SYS_PROC_INFO => sys_proc_info(a1 as usize, a2, a3 as usize),
        SYS_MEM_STATS => sys_mem_stats(),
        SYS_KILL => sys_kill(a1),
        SYS_FS_COUNT => crate::fs::get_all_vfs_paths().len() as u64,
        SYS_FS_NAME => sys_fs_name(a1 as usize, a2, a3 as usize),
        SYS_FS_READ => sys_fs_read(a1, a2, a3, _a4 as usize),
        SYS_BEEP => sys_beep(a1 as u32, a2 as u32),
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
    // Actually give the CPU up rather than returning and letting the caller spin
    // out the rest of its quantum. `hlt` parks the core until the next timer
    // tick, which rotates to the next ready task.
    //
    // This enables interrupts inside a syscall, so the handler is preemptible
    // from here. That is safe for two reasons: the user `rsp` is saved on this
    // task's kernel stack rather than in a global, so another process entering
    // a syscall in the gap cannot corrupt this one's return; and the idle task
    // already halts this way in Ring 0, so being interrupted in kernel context
    // is a path the scheduler handles.
    unsafe {
        core::arch::asm!("sti; hlt; cli", options(nomem, nostack));
    }
    0
}

// -----------------------------------------------------------------------------
// Window surface, process and filesystem services
// -----------------------------------------------------------------------------

/// Maps this process's window surface and returns its user base address.
fn sys_surface_map() -> u64 {
    use crate::memory::read_cr3;
    let pid = crate::task::current_pid();
    match crate::gui::surface::map_for_process(read_cr3(), pid) {
        Some(addr) => addr,
        None => SysError::Fault as i64 as u64,
    }
}

/// Writes `"<pid> <state> <cpu%> <name>"` for the process at `index` into a user
/// buffer. Returns the byte count written, or an error.
fn sys_proc_info(index: usize, out_ptr: u64, out_len: usize) -> u64 {
    use crate::task::pcb::TaskState;

    let processes = crate::task::get_process_list();
    let Some(info) = processes.get(index) else {
        return SysError::Invalid as i64 as u64;
    };

    let state = match info.state {
        TaskState::Running => "RUN",
        TaskState::Ready => "RDY",
        TaskState::Blocked(_) => "BLK",
        TaskState::Terminated(_) => "DEAD",
        TaskState::Zombie => "ZOMB",
    };

    let mut buf = [0u8; 128];
    let mut writer = SliceWriter::new(&mut buf);
    let _ = core::fmt::write(
        &mut writer,
        format_args!("{} {} {} {}", info.pid, state, info.cpu_percent, info.name),
    );
    let written = writer.written();

    let count = core::cmp::min(written, out_len);
    if !copy_to_user(out_ptr, &buf[..count]) {
        return SysError::Fault as i64 as u64;
    }
    count as u64
}

/// Returns `(used_bytes << 32) | total_megabytes`, both saturated to fit.
fn sys_mem_stats() -> u64 {
    let (used_bytes, total_bytes) = crate::task::get_memory_stats();
    let used_kb = (used_bytes / 1024).min(u32::MAX as u64);
    let total_kb = (total_bytes / 1024).min(u32::MAX as u64);
    (used_kb << 32) | total_kb
}

/// Terminates another process. PID 0 (the idle task) is refused, matching the
/// protection the in-kernel `kill` command already has.
fn sys_kill(pid: u64) -> u64 {
    if pid == 0 {
        return SysError::Invalid as i64 as u64;
    }
    if crate::task::kill_process(pid) {
        0
    } else {
        SysError::Invalid as i64 as u64
    }
}

/// Writes the VFS path at `index` into a user buffer.
fn sys_fs_name(index: usize, out_ptr: u64, out_len: usize) -> u64 {
    let paths = crate::fs::get_all_vfs_paths();
    let Some(path) = paths.get(index) else {
        return SysError::Invalid as i64 as u64;
    };

    let bytes = path.as_bytes();
    let count = core::cmp::min(bytes.len(), out_len);
    if !copy_to_user(out_ptr, &bytes[..count]) {
        return SysError::Fault as i64 as u64;
    }
    count as u64
}

/// Reads a VFS file into a user buffer. Returns bytes read, or an error.
fn sys_fs_read(path_ptr: u64, path_len: u64, out_ptr: u64, out_len: usize) -> u64 {
    let mut path_buf = [0u8; 256];
    let Some(path) = read_user_str(&mut path_buf, path_ptr, path_len as usize) else {
        return SysError::Fault as i64 as u64;
    };

    let Ok(contents) = crate::fs::read_file(path) else {
        return SysError::Invalid as i64 as u64;
    };

    let count = core::cmp::min(contents.len(), out_len);
    if !copy_to_user(out_ptr, &contents[..count]) {
        return SysError::Fault as i64 as u64;
    }
    count as u64
}

/// Plays a tone on the PC speaker. Bounded so a user process cannot pin the
/// speaker on indefinitely or program a nonsense divisor.
fn sys_beep(freq_hz: u32, duration_ms: u32) -> u64 {
    if !(20..=20_000).contains(&freq_hz) {
        return SysError::Invalid as i64 as u64;
    }
    crate::drivers::speaker::beep(freq_hz, duration_ms.min(1000));
    0
}

/// `core::fmt::Write` into a fixed slice, so the handlers above can format
/// without allocating. Interrupts are masked here; the global allocator is a
/// plain spinlock, and taking it under a mask is exactly what `PROJECT.md`
/// warns against.
struct SliceWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> SliceWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn written(&self) -> usize {
        self.pos
    }
}

impl core::fmt::Write for SliceWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let remaining = self.buf.len() - self.pos;
        let count = core::cmp::min(remaining, s.len());
        self.buf[self.pos..self.pos + count].copy_from_slice(&s.as_bytes()[..count]);
        self.pos += count;
        Ok(())
    }
}
