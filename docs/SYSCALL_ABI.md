# Userspace ABI

The interface between Ring 3 and the AegisOS kernel. Before this existed, a user
process could compute and it could fault, and nothing else — the only userspace
in the system was a set of payloads designed to crash. A program can now be
loaded from an ELF image and can call into the kernel.

## Calling convention

Modelled on System V / Linux x86_64 so that ordinary toolchain output can be
used later without inventing a private convention.

| Register | Role |
|---|---|
| `rax` | syscall number in, return value out |
| `rdi` | argument 1 |
| `rsi` | argument 2 |
| `rdx` | argument 3 |
| `r10` | argument 4 |
| `r8`  | argument 5 |
| `rcx`, `r11` | clobbered by the `syscall` instruction itself |

Every other register is preserved, matching what Linux promises and therefore
what any toolchain targeting this convention assumes. The entry stub saves the
argument registers as well as the callee-saved set, because the Rust dispatcher
it calls treats the argument registers as scratch. Getting this wrong is not
subtle and not loud: the Ring 3 terminal's first symptom was a struct-return
pointer in `rdi` being destroyed across an unrelated syscall.

Return values `>= 0` are results. Negative values are errors:

| Value | Name | Meaning |
|---|---|---|
| `-1` | `NoSys` | unknown syscall number |
| `-2` | `Fault` | a pointer argument was not a readable user mapping |
| `-3` | `Invalid` | an argument was out of range |

## Calls

| № | Name | Signature | Notes |
|---|---|---|---|
| 0 | `exit` | `exit(code: i64) -> !` | Marks the process terminated and yields. Never returns. |
| 1 | `write` | `write(fd, buf, len) -> len` | `fd` 1 and 2 both reach the serial console. `len` is capped at 4096. There is no file descriptor table yet. |
| 2 | `yield` | `yield() -> 0` | Gives up the rest of the time slice. |
| 3 | `getpid` | `getpid() -> pid` | |
| 4 | `uptime` | `uptime() -> ticks` | 100 Hz timer ticks since boot. |
| 5 | `surface_map` | `surface_map() -> base` | Maps this process's window surface. See below. |
| 6 | `surface_dims` | `surface_dims() -> (w<<32)\|h` | Surface dimensions. |
| 7 | `poll_event` | `poll_event() -> packed` | Next key or mouse event, or 0. |
| 8 | `proc_count` | `proc_count() -> n` | |
| 9 | `proc_info` | `proc_info(i, buf, len) -> n` | Writes `"<pid> <state> <cpu%> <mem_kib> <name>"`, name last. |
| 10 | `mem_stats` | `mem_stats() -> (used_kib<<32)\|total_kib` | |
| 11 | `kill` | `kill(pid) -> 0` | PID 0 is refused. |
| 12 | `fs_count` | `fs_count() -> n` | VFS entries. |
| 13 | `fs_name` | `fs_name(i, buf, len) -> n` | Writes the path at `i`. |
| 14 | `fs_read` | `fs_read(path, plen, buf, len) -> n` | |
| 15 | `beep` | `beep(hz, ms) -> 0` | 20..20000 Hz, capped at 1000 ms. |
| 16 | `spawn_fault` | `spawn_fault(kind) -> pid` | Spawns a process that faults on purpose. See below. |
| 17 | `cpu_usage` | `cpu_usage() -> percent` | System-wide CPU utilisation, 0..100. |

Example, from `src/task/userprogs.rs`:

```asm
    mov rax, 1              # SYS_WRITE
    mov rdi, 1              # fd = stdout
    lea rsi, [rip + msg]
    mov rdx, msg_end - msg
    syscall

    mov rax, 0              # SYS_EXIT
    mov rdi, 0
    syscall
```

## How the transition works

`syscall` does not consult the TSS the way an interrupt does — it leaves `rsp`
pointing at the *user* stack and hands control to whatever `IA32_LSTAR` names.
The entry stub in `src/arch/syscall.rs` therefore switches stacks itself before
touching anything.

MSRs programmed at boot by `init_syscall`:

| MSR | Value | Why |
|---|---|---|
| `IA32_EFER` | `SCE` + `NXE` | Without `SCE`, `syscall` raises #UD. `NXE` makes bit 63 of a page table entry mean no-execute instead of reserved. |
| `IA32_STAR` | `(0x10 << 48) \| (0x08 << 32)` | Selector bases. `syscall` loads `CS = STAR[47:32]`, `SS = STAR[47:32] + 8`; `sysretq` loads `CS = STAR[63:48] + 16`, `SS = STAR[63:48] + 8`, forcing RPL 3. The existing GDT layout (kernel code `0x08`, kernel data `0x10`, user data `0x18`, user code `0x20`) already satisfies both. |
| `IA32_LSTAR` | `syscall_entry` | Kernel entry point. |
| `IA32_FMASK` | TF, IF, DF, NT, AC | Cleared on entry. |

## User pointer validation

Any pointer from Ring 3 is treated as hostile. `copy_from_user` rejects a range
that leaves the lower half or wraps the address space, then walks the page tables
for each page, requiring `USER_ACCESSIBLE` at *every* level of the walk.

The level-by-level check is the part that matters. A lower-half bound check
alone would already exclude the kernel's own mappings in this layout, but
checking the U bit at each level makes the guarantee independent of where the
kernel happens to place things, and it is what stops a process naming a
supervisor page that its own address space maps.

## Two deliberate limitations

**Single CPU.** The entry stub parks the user `rsp` in a static and loads the
kernel stack from another, addressed RIP-relative, rather than using `swapgs` and
a per-CPU block. That is correct only while one core at a time can be inside the
stub. The kernel has no SMP support, so it holds today. Adding SMP means moving
those two statics into a per-CPU structure reached through
`IA32_KERNEL_GS_BASE` and pairing `swapgs` on entry and exit.

**Syscalls are not preemptible.** `IA32_FMASK` clears `IF`, so a handler runs to
completion with interrupts masked. This sidesteps the lock-ordering hazard in
`PROJECT.md` — a handler cannot be interrupted while holding a lock an ISR also
takes — at the cost of adding handler runtime to interrupt latency. Handlers must
stay short; `write` caps its buffer at 4 KiB for exactly this reason.

## Drawing: window surfaces

A Ring 0 app draws by being handed `&mut Framebuffer`. A Ring 3 app cannot be
handed a kernel pointer, so it gets a surface: a block of pixels the kernel
owns, mapped writable (and `NO_EXECUTE`) into the process at `0x1000_0000`,
which the compositor blits into that window's client rect each frame.

There is deliberately **no syscall that draws**. Every pixel a user process puts
on screen goes through memory it owns, so a confused or hostile process can
corrupt its own window and nothing else — the kernel only ever reads the
surface, and clips the blit to the smaller of the surface and the client rect.

The surface is 640x384 ARGB, fixed rather than negotiated: resizing a window
shows more or less of it instead of requiring a realloc and a protocol to
announce the new size. Up to four processes hold one at a time, keyed by PID; each is allocated on
first use and mapped at the same user address in every address space, since each
process has its own.

Compositing one is not free: a window costs a full 640x384 blit every frame.
Three open at once cut the frame rate by about a third, which is why Ring 3 apps
start when launched rather than at boot, and why the blit is a clipped row copy
rather than a per-pixel loop.

The compositor never holds the surface lock across a blit. It copies the frame
list under a brief guarded lock and reads pixels without one — holding it would
deadlock against `SYS_SURFACE_MAP`, which takes the same lock from syscall
context where `IF` is already masked and so cannot be preempted out of the way.

## Input: event polling

Input for a focused Ring 3 window is packed into a `u64` and queued in
`src/task/uevent.rs`, collected with `SYS_POLL_EVENT`. Two event types share the
encoding, tagged in the top byte:

```text
 key   (type 1):  bits 55..40  key code (ASCII, or 0x100.. for Enter, arrows, ...)
                  bits   2..0  alt, ctrl, shift

 mouse (type 2):  bits 55..40  x within the client area
                  bits 39..24  y within the client area
                  bits 23..16  button (0 left, 1 right, 2 middle)
                  bits  15..8  action (0 move, 1 down, 2 up)
```

Mouse coordinates are **client-relative**. The compositor does the translation,
because only it knows where the window is, and a process therefore never learns
where it sits on screen or receives a coordinate outside its own surface —
events outside are dropped rather than clamped.

A window's client area belongs entirely to its process: the window manager takes
the titlebar and the traffic lights before dispatch, and everything inside goes
to Ring 3. Motion is delivered whenever the pointer is over the client area,
dragging or not, so a process can implement hover.

Trailing motion events coalesce: the PS/2 stream produces far more moves than a
process redrawing at frame rate can consume, and a queue full of stale positions
would push out the button events behind them. Replacing a trailing move keeps
the newest position without displacing anything else.

The queue is a fixed-capacity ring, not a `VecDeque`, for the reason
`PROJECT.md` gives: it is written from the compositor loop and read from syscall
context with interrupts masked, and a growable collection allocates on push. It
drops the oldest event when full, so a process that stops polling cannot stall
the compositor.

## `spawn_fault`, and what it costs

`SYS_SPAWN_FAULT` asks the kernel to create a Ring 3 process that faults on
purpose: 0 null dereference, 1 divide by zero, 2 write into kernel space,
3 invalid opcode. It exists so the Crash-Test demo can live in Ring 3 — the
app's whole point is watching *another* process die while the desktop keeps
running, which it cannot demonstrate by faulting itself.

Two things about it are worth stating plainly rather than discovering later.
It is a process-creation primitive exposed to userspace with **no permission
model**: any Ring 3 process can call it, which is tolerable only because this
kernel has no notion of privilege beyond the ring boundary yet. And it does real
work — a kernel stack, an address space, several frames — with interrupts
masked, adding that time to interrupt latency. Both are acceptable for a demo
behind a button press and would not be for a general `spawn`.

## ELF loading

`src/task/elf.rs` handles `ET_EXEC` ELF64 images for `EM_X86_64`. Position
independent executables are not supported: they need relocation processing and a
dynamic loader, neither of which exists.

Validation before anything is mapped: magic, class, endianness, type, machine,
program header table bounds, `filesz <= memsz`, segment extent inside the file,
segment range inside user space and clear of the user stack, a total image
budget of 256 pages, and an entry point that lands inside a `PT_LOAD` segment.

Segment permissions are carried into the page tables — a segment without `PF_W`
is mapped read-only, and one without `PF_X` is mapped `NO_EXECUTE`. This is
stricter than `spawn_user_bytecode`, which maps its single page writable *and*
executable.

Frames are recorded in the PCB as they are allocated, so a rejected image
releases everything it touched, and a loaded one is reclaimed by the existing
two-phase zombie reaper when the process dies.

## What this does not do yet

- No `fork`, `exec`, `open`, `mmap` or `brk`. A process gets one address space,
  a 64 KiB stack, and the calls in the table above.
- No file descriptor table; `write` special-cases 1 and 2, and there is no
  `fs_write` yet — the Ring 3 terminal can read the VFS but not modify it.
- No signals, no threads, no IPC.
- No `vDSO`, no TLS, no `arch_prctl`, so `fs`/`gs` bases are unset.
- Mouse wheel and double-click are not delivered; the driver has no notion of
  either.
- No guard page below the stack. An overflow faults, which is how the Ring 3
  terminal's first bug was found, but it faults into whatever is mapped below
  rather than reliably into a hole.
- Eleven of the fourteen desktop applications still run in Ring 0. The Terminal,
  Crash-Test and Activity Monitor have moved; see `GOALS.md` for the rest.
