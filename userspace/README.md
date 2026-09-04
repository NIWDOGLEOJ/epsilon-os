# Ring 3 userspace

Programs that run as user processes on AegisOS, outside the kernel's privilege
level and outside its address space.

Three programs, sharing one library. Neither is kernel code: they cannot call a
kernel function, cannot read kernel memory, and reach system state only through
the syscalls documented in [`../docs/SYSCALL_ABI.md`](../docs/SYSCALL_ABI.md).

| File | |
|---|---|
| `src/bin/terminal.rs` | The Terminal, ported from `src/apps/terminal.rs` |
| `src/bin/crash_test.rs` | Crash-Test, ported from `src/apps/crash_test.rs` |
| `src/bin/activity_monitor.rs` | Activity Monitor, ported from `src/apps/activity_monitor.rs` |
| `src/lib.rs` | Shared library root |
| `src/sys.rs` | Syscall shims |
| `src/surface.rs` | Drawing into the window surface the kernel maps |
| `src/font.rs` | Text rendering, sharing glyph data with the kernel |
| `src/text.rs` | Colours and allocation-free formatting |
| `src/rt.rs` | `_start` and the panic handler — included per binary, not in the library, so each program gets exactly one of each |
| `linker.ld` | Links at `0x400000`, lower half, three segments |

## Building

`build.rs` at the repository root builds this automatically as part of the
kernel, so a plain `cargo build` is enough. It is not a workspace member: it
needs its own linker script, `code-model=small` instead of the kernel's
`code-model=kernel`, and a separate target directory (nested cargo sharing a
target directory deadlocks on the parent's lock).

To build it alone:

```sh
CARGO_ENCODED_RUSTFLAGS=$'-C\x1flink-arg=-T'"$PWD/linker.ld"$'\x1f-C\x1frelocation-model=static\x1f-C\x1fcode-model=small\x1f-C\x1fno-redzone=y\x1f-C\x1ftarget-feature=-sse,-sse2,-avx,-avx2' \
  cargo build --release --target x86_64-unknown-none --target-dir ../target/userspace
```

## Constraints

**No allocator.** Everything is fixed-size buffers. There is no heap in Ring 3
yet, so no `String`, no `Vec`, and no `format!` — hence the hand-rolled integer
formatting in `main.rs`.

**No SSE.** Compiled with `-sse,-sse2,-avx,-avx2`, matching the kernel. The
scheduler saves general-purpose registers only, so any FPU or vector state held
across a context switch would be silently corrupted.

**64 KiB of stack, no guard page.** Enough for the fixed buffers here; an
overflow faults into whatever is mapped below rather than reliably into a hole.

**Surfaces are 640x384.** The kernel maps one per process, up to four at once.
A window costs a full-surface blit every frame, so these apps start when
launched -- from Spotlight, or `run r3term` / `r3fault` / `r3proc` in the
desktop Terminal -- rather than at boot.

## Toolbar

A row of clickable buttons across the top of the surface — `help`, `ps`, `free`,
`ls`, `clear` and `crash` — with hover highlighting. Clicking one feeds the same
dispatcher the keyboard uses, so a click is indistinguishable from typing the
command.

It exists partly as a demonstration: hover proves motion events arrive, and
clicking `crash` proves a pointer event can reach Ring 3, be resolved to a
control by user code, and kill the process without disturbing the desktop.

## Commands

`help`, `echo`, `clear`, `ps`, `free`, `kill`, `ls`, `cat`, `uptime`, `pid`,
`beep`, `exit`, plus `crash` and `panic`, which exist to be run: they kill this
process and leave the desktop composing.

This is a subset of the Ring 0 terminal's command set. The missing ones need ABI
that does not exist yet — writing to the VFS, launching kernel windows, changing
the wallpaper. See `GOALS.md`.
