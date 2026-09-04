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

- No `fork`, `exec`, `open`, `read`, `mmap` or `brk`. A process gets one address
  space, one stack, and the calls in the table above.
- No file descriptor table; `write` special-cases 1 and 2.
- No signals, no threads, no IPC.
- No `vDSO`, no TLS, no `arch_prctl`, so `fs`/`gs` bases are unset.
- Applications still run in Ring 0 (see `GOALS.md`). This ABI is what makes
  moving them out possible; it does not by itself move them.
