# AegisOS Test Suite Status

**Status:** the two suites that exercise the real kernel are `tests/qemu_e2e/` and
`src/selftest/`. This document also records `tests/e2e/`, a host-side design model that
does **not** test the kernel. Read section 3 before quoting any number from it.

---

## 1. Suites that test the kernel

### 1.1 QEMU E2E suite — `tests/qemu_e2e/`

Boots the real `aegis_os.iso` in QEMU and drives it from outside: keystrokes and mouse
events through the QEMU monitor, COM1 serial parsing, PPM framebuffer screendumps, and
`info registers` sampling to confirm `RFLAGS.IF` stays set and `RIP` keeps advancing.

```sh
./run_e2e_tests.sh                       # builds the ISO if stale, then runs everything
python3 -m tests.qemu_e2e.runner --help  # runner options
```

22 test functions across 20 modules: boot, framebuffer, fault isolation (2), terminal,
terminal advanced, stability (2), selftest, frame pacing, VFS, paint, file manager,
audio, window snapping, settings, calculator, Spotlight/browser, minesweeper, editor
advanced, synth, chat.

### 1.2 In-kernel self-tests — `src/selftest/`

Compiled in behind `--features selftest`, run at early boot before the desktop starts,
and exit QEMU deterministically through `isa-debug-exit` on port `0xf4` — QEMU status
33 (`0x21`) for pass, 35 (`0x23`) for fail.

```sh
./run_selftest.sh
```

14 suites, in execution order: physical frame allocator; PML4 paging & address-space
isolation; kernel dynamic heap; task scheduler lifecycle; in-memory VFS; PC speaker;
wallpaper & PPM parser; scientific calculator; terminal engine; agent/Spotlight/browser;
minesweeper; AegisPad 2.0; AegisSynth; virtual network & AegisChat.

---

## 2. What has actually been verified

Claims below trace to booting the ISO and reading serial output, framebuffer
screendumps, or `rdtsc` counters — see HANDOFF.md for the measurement method.

1. **Ring 3 fault isolation.** #PF (null and out-of-bounds), #DE, #UD, and a Ring 3
   write into kernel space are each trapped by hardware, logged with vector, RIP, error
   code and CR2, and the faulting process is reaped. The desktop keeps compositing.
   Two-phase deferred reclamation returns the zombie's frames to the bitmap allocator.
2. **Memory footprint.** 16 MB used of 3064 MB at idle desktop, inside the < 60 MB
   budget.
3. **Timer accuracy.** The PIT is programmed to 100 Hz and the uptime clock tracks wall
   time (19s → 50s over 31s measured).
4. **Input under load.** Keyboard and mouse survive a 560-event flood with interrupts
   still enabled and RIP advancing.
5. **Resolution independence.** Boots and composites at 1280x800 and at 800x600.

HANDOFF.md records the suites at 22/22 and 14/14 as of milestone 17. That is a claim
from that log; run the two scripts above for numbers from your own checkout.

---

## 3. `tests/e2e/` — a design model, not a test suite

`tests/e2e/` is ~6,700 lines of standard-library Rust that reimplement the kernel's
data structures on the host. No file in it references the kernel crate:

```
$ grep -rn 'use aegis_os\|aegis_os::' tests/
  -> 0 files
```

Every file declares its own `PhysAddr`, `VirtAddr`, `PAGE_SIZE`, `TaskState` and
`Scheduler`. It is not wired into the build, and cannot easily be — the kernel is
`no_std` for `x86_64-unknown-none`.

Earlier revisions of this document opened with **"Overall Pass Rate: 100% (135/135
tests passed)"** and **"tear-free 60 FPS verified"**. Both were reported while the
kernel deadlocked fifteen lines into boot and had never rendered a single frame; the
real frame rate at the time was 3. The models passed because they never boot, link, or
call the kernel. That headline is the reason nobody noticed the kernel did not boot,
and it is why it has been removed rather than updated.

What the tree still contains, as a record of the original design:

| File | Models |
|---|---|
| `tests/e2e/tier1_features.rs` | Feature coverage for F1–F12 (61 cases) |
| `tests/e2e/tier2_boundary.rs` | Boundary and corner cases (61 cases) |
| `tests/e2e/tier3_combinations.rs` | Pairwise subsystem interactions (8 cases) |
| `tests/e2e/tier4_scenarios.rs` | Multi-step workflows (5 cases) |
| `tests/e2e/test_harness/memory_sim.rs` | Bitmap frame allocator & PML4 paging |
| `tests/e2e/test_harness/privilege_sim.rs` | GDT, TSS, IDT & UART logger |
| `tests/e2e/test_harness/scheduler_sim.rs` | Round-robin scheduler & zombie reaper |
| `tests/e2e/test_harness/gui_sim.rs` | Double-buffered compositor & font renderer |
| `tests/e2e/test_harness/input_sim.rs` | PS/2 scancode & mouse packet decoders |
| `tests/e2e/test_harness/wm_sim.rs` | Window manager, menu bar, dock |
| `tests/e2e/test_harness/apps_sim.rs` | The original five applications |
| `tests/e2e/test_harness/os_kernel_env.rs` | Unified simulation environment |

It describes the five-app desktop of M4 and has not tracked the fourteen-app system
since. HANDOFF.md proposes moving it to `docs/model/` or deleting it; that has not been
done.

It still runs on the host if you want to read it executing:

```sh
cargo test --manifest-path tests/e2e/Cargo.toml
cargo run  --manifest-path tests/e2e/Cargo.toml --bin e2e_runner
```

A pass there says the model is self-consistent. It says nothing about `src/`.

---

## 4. Related documents

- [`TEST_INFRA.md`](TEST_INFRA.md) — framework design and coverage matrix.
- [`HANDOFF.md`](HANDOFF.md) — measurement method, per-milestone test additions, and
  the open question of what to do with `tests/e2e/`.
- [`README.md`](README.md) — short version of all of the above.
