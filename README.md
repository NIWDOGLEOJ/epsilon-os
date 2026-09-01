# Epsilon OS (AegisOS kernel)

A crash-resilient x86_64 operating system in `no_std` Rust, with hardware Ring 0/Ring 3
fault isolation and a double-buffered graphical desktop.

The kernel identifies itself as **AegisOS**; `epsilon-os` is the repository name.

![desktop](docs/desktop.png)

## What it is

~7,800 lines of bare-metal Rust: Limine boot into a higher-half kernel, GDT/TSS/IDT,
4-level PML4 paging, a bitmap frame allocator and 16 MB kernel heap, a 100 Hz
preemptive round-robin scheduler, and a software compositor driving a macOS-style
desktop with seven applications.

The headline feature is fault isolation. A Ring 3 process that dereferences null,
divides by zero, executes `ud2`, or writes into kernel space is trapped by hardware,
logged, and reaped — the desktop keeps running:

```
[FAULT-ISOLATION] Userspace Fault caught: Page Fault (#PF) (vec 14) at RIP=0x400000, ErrorCode=0x6, CR2=0x0
[FAULT-ISOLATION] Process PID 9 ('crash_null_ptr') crashed due to Page Fault (#PF)
[FAULT-TELEMETRY] Ring 3 Task PID 9 caught Page Fault (#PF). Desktop remains 100% stable.
```

The Crash-Test app has a button for each of those four faults. They all work.

## Build and run

Needs a Rust toolchain with the `x86_64-unknown-none` target, plus `qemu-system-x86_64`
and `xorriso`.

```sh
rustup target add x86_64-unknown-none
./run_qemu.sh                 # builds the kernel, packages the ISO, boots it
./build_iso.sh                # just produce aegis_os.iso
```

Headless, with serial on stdout:

```sh
qemu-system-x86_64 -cdrom aegis_os.iso -m 4G -accel kvm -vga std \
                   -display none -serial stdio
```

The ISO is a build artifact and is not committed.

## Applications

Terminal (`help`, `ps`, `kill`, `free`, `run`, `echo`, `neofetch`, `clear`, `reboot`),
Activity Monitor, AegisPad editor, Crash-Test, Calculator, Snake, and an About dialog.
Launch from the dock or with `run <app>`.

## Measured state

Numbers from QEMU (`-accel kvm`, 1280x800x32) — serial logs, framebuffer screendumps
and `rdtsc`, not estimates.

| | |
|---|---|
| Compositor | ~72 FPS |
| Frame cost | ~28 Mcyc |
| Uptime clock | accurate to wall time |
| Timer | 100 Hz (PIT-programmed) |
| Memory at idle | 16 MB of 3064 MB |
| Fault isolation | #PF, #DE, #UD and Ring 3 → kernel-space write all trapped and reaped |
| Stability | 560-event input flood + soak, interrupts stay enabled |

Resolution is not hardcoded; verified booting at both 1280x800 and 800x600.

## Reading order

- [`HANDOFF.md`](HANDOFF.md) — what was broken, what was fixed, and what to do next.
  Start here.
- [`PROJECT.md`](PROJECT.md) — architecture, feature inventory, and the two
  engineering invariants the kernel depends on.
- `src/arch/mod.rs` and `src/drivers/ring.rs` — those two invariants in code.

## A caveat about `tests/`

`tests/` contains ~6,700 lines that do **not** reference the kernel crate. They model
the design in `std` Rust rather than exercising `src/`. `TEST_READY.md` reports
"135/135 passed" on that basis. Treat both as design notes, not evidence — see
HANDOFF.md. Everything asserted in this README was verified by booting the thing.
