# Handoff

Engineering log for the debugging and repair pass on this kernel, and the plan from here.

Written after taking the kernel from "hard-deadlocks 15 lines into boot" to a working
72 FPS desktop. Every claim below was verified by booting the ISO in QEMU and reading
serial output, framebuffer screendumps, or `rdtsc` counters.

## Starting state

The kernel deadlocked during boot. A framebuffer screendump was a single flat colour —
zero frames had ever rendered. Nothing in the graphics, desktop, or application layers
was observable, though all of it was written and compiled.

`PROJECT.md` marked M3/M4/M5 `PLANNED` and M1/M2 `DONE`. Both were wrong: the graphics
and desktop code existed and worked once the hang was cleared, while the "done"
fault-isolation engine was what hung the machine.

## Bugs found and fixed

### 1. Serial spinlock deadlock — `arch/serial.rs`

`_print` took a `spin::Mutex` with interrupts enabled. The boot sequence enabled
interrupts, began logging, and took the lock; the timer IRQ switched to a Ring 3 task
that faulted on purpose; the fault handler tried to log the fault and spun forever on
a lock the interrupted compositor still held.

Diagnosed from the CPU state at the hang: `RFLAGS=0x2` (IF clear), `RIP` pinned across
five samples in a `lock cmpxchg` retry loop, and an interrupt frame on the stack
showing vector `0x0e`, error `0x06`, `CS=0x23`.

Fixed with `arch::InterruptGuard` — saves `RFLAGS`, clears `IF`, restores on drop.

### 2. Mouse acceleration — `drivers/ps2_mouse.rs`

`scale_delta` applied `abs*6 + abs²/6` above delta 10, turning a single 50-count packet
into 716 pixels and pinning the cursor to a screen edge on any normal movement.
Replaced with a piecewise curve: 1:1 below 5 counts, 2x to 10, 2.5x above.

Measured travel per packet: 2 → 2 px, 10 → 16 px, 50 → 116 px (was 14/56/716).

### 3. No clipping — `drivers/framebuffer.rs`

Nothing scissored application drawing, so Terminal text painted across the Activity
Monitor window. Added a clip rect enforced in `draw_pixel`, the single choke point every
primitive, the font rasterizer and the cursor route through. Nested clips intersect;
a non-overlapping intersection collapses to an empty rect rather than `None`, which
would have meant "unrestricted".

### 4. Z-order — `gui/wm.rs`, `main.rs`

`render_desktop` drew every window *frame*, then `main.rs` drew every window *content*
in a second pass — so a lower window's content landed on a higher window's frame.
`render_desktop` now takes a `render_client` callback and interleaves frame-then-content
per window, each clipped to its own client rect.

### 5. Compositor at ~3 FPS

Profiled rather than guessed. Per-frame: 968 Mcyc total — windows 739, wallpaper 177,
menubar/dock 52, and the VRAM blit **1**. The "tear-free 60 FPS scanline blit" was
already 0.1% of the frame; all the time was in per-pixel software rasterization at
**173 cycles per wallpaper pixel**.

Three fixes:

- `Framebuffer::fill_span` — bounds, clip and dirty-tracking once per horizontal span,
  `slice::fill` for opaque spans. `draw_rect` and `draw_gradient_v` emit one span per row.
- `rounded_row_span` — solves the corner circle per row instead of testing all four
  corners against every pixel. Identical output, one span at a time.
- `draw_shadow` — was drawing six concentric full-size rounded rects per window,
  ~1M alpha-blended pixels of which nearly all sat behind the opaque window body. Now
  takes `occluded_by` and skips what a following opaque body will cover. Only windows
  pass `Some`; the dock, toast and tooltip bodies are translucent and pass `None`.

Then `[profile.dev] opt-level` 1 → 3, since `build_iso.sh` ships the dev profile.

968 Mcyc → ~28 Mcyc per frame. ~3 FPS → ~72 FPS.

### 6. The 100 Hz timer was never 100 Hz — `arch/idt.rs`

Nothing programmed the PIT. IRQ 0 ran at the 8254 power-on default of ~18.2 Hz, so the
documented "10 ms" scheduler quantum was really 55 ms and every tick-derived value was
out by 5.5x. Added `init_pit()` with `TIMER_HZ = 100`.

The desktop clock had been `frame_count / 60`, making it a function of rendering speed.
It now reads `get_uptime_ticks() / TIMER_HZ`. Verified 19 → 50 over 31 wall seconds.

### 7. Interrupt-safety audit — the same bug three more times

Auditing every `static Mutex` against the four ISR entry points found `MOUSE_DRIVER`,
`MOUSE_QUEUE`, `KEYBOARD_STATE` and `KEY_QUEUE` all locked from the compositor loop with
interrupts enabled while their IRQ handlers locked the same statics. Raising the frame
rate had made these ~20x more likely by increasing the poll rate.

It also found something a guard cannot fix: **the interrupt handlers were allocating.**
`on_mouse_irq` returned a freshly allocated `VecDeque` per byte — 600 allocations/sec at
the configured 200 Hz sample rate — and `handle_user_fault` cloned a `String`. The global
allocator is a plain spinlock, so an ISR allocating while the interrupted code is inside
the allocator hangs the machine.

Added `drivers/ring.rs`: `EventRing<T: Copy, N>`, preallocated in the static itself, now
backing both input queues and the scheduler's zombie queue. No ISR path allocates.

### 8. Font — `gui/font.rs`

`—` rendered as `???` and `🛡️` as `?????`. Not missing glyphs: `draw_string` iterated
`text.bytes()`, so every byte of a multi-byte character fell back to `?` individually.
Switched to `chars()` and added glyphs for all eight non-ASCII codepoints in the tree —
found by scanning source, which is how the Calculator's broken `×` `÷` `±` keys turned
up in a window nobody had opened.

### Smaller fixes

- Crash-Test's status line was anchored to `client.height - 24`, placing it inside the
  fourth button. Now sits below the buttons; window grew 270 → 300 to fit.
- The About dialog hardcoded `1024x768x32`. Now reads the live mode off the `fb`
  reference — note that calling `framebuffer::get_dimensions()` there would
  self-deadlock, since the compositor already holds that lock while rendering.
  Verified at 1280x800 and 800x600.
- `menubar.rs` advanced the pen by `menu.len() * 8` (byte length, not codepoints).

## Invariants

Two rules the kernel now depends on. Breaking either reintroduces a hard hang.

**1. Any `static Mutex` an ISR touches may only be locked from task context under an
`arch::InterruptGuard`.** Contention between two tasks resolves — the spinning task gets
preempted and the holder runs. Contention from an ISR does not: the handler runs with
`IF` clear, so the holder can never be rescheduled to release the lock.

Applies to `SERIAL1`, `SCHEDULER`, `CRASH_CALLBACK`, `KEYBOARD_STATE`, `KEY_QUEUE`,
`MOUSE_DRIVER`, `MOUSE_QUEUE`. `GLOBAL_FRAME_ALLOCATOR` and `FRAMEBUFFER` have no ISR
user. ISR handlers themselves take no guard — they already run with interrupts masked.

**2. Interrupt handlers must not allocate.** `Vec` and `VecDeque` grow on push and are
unusable in an ISR. Use `drivers::ring::EventRing`.

## How this was verified

There is no automated suite (see below). Everything was checked by driving QEMU:

- headless boot with `-serial file:` and `-monitor unix:` for a control socket
- `sendkey` / `mouse_move` / `mouse_button` through the monitor to drive real input
- `screendump` to PPM, then pixel inspection for cursor position, clipping and glyphs
- `info registers` for `RFLAGS.IF` and `RIP` sampling to tell a deadlock from a busy loop
- `addr2line` against the kernel ELF to resolve a faulting or spinning `RIP` to source
- a 560-event input flood plus soak runs to exercise the ISR paths under load

## What to do next

### 1. Build the QEMU E2E harness — highest value

Everything in the section above was done by hand. Scripted, it becomes a regression
suite that would have caught all eight bugs on day one. Concrete assertions:

- boot reaches `AegisOS macOS Desktop Compositor Active` within N seconds
- framebuffer is not a single flat colour after boot
- all four Crash-Test buttons produce the right vector in the serial log
- desktop clock tracks wall time within tolerance
- after a synthetic input flood, `RFLAGS.IF` is still set and `RIP` still moves
- no `?` glyph clusters where UI strings contain non-ASCII

The plumbing already exists in `build_iso.sh` plus a monitor socket; this is mostly
scripting what is documented above.

### 2. In-kernel self-tests

A `selftest` feature that asserts against the real frame allocator, paging and scheduler
at boot, reports over serial, and exits QEMU through `isa-debug-exit` with a status code.
The standard `no_std` pattern, and it reaches unit-level behaviour E2E cannot.

### 3. Deal with `tests/` and `TEST_READY.md`

`tests/` is ~6,700 lines that never reference the kernel crate:

```
$ grep -rn 'use aegis_os\|aegis_os::' tests/
  -> 0 files
```

Every file declares its own `PhysAddr`, `VirtAddr`, `PAGE_SIZE`, `TaskState`,
`Scheduler` in `std` Rust. It is a model of the design, not a test of `src/`, and it is
not wired into the build — nor can it easily be, since the kernel is `no_std` for
`x86_64-unknown-none`.

`TEST_READY.md` reports "100% (135/135 tests passed)" and "tear-free 60 FPS verified" on
that basis. Those claims coexisted with a kernel that deadlocked on boot and rendered
zero frames, because the tests never boot, link, or call it. The real frame rate at the
time was 3.

Move it to `docs/model/` and say what it is, or delete it — but the pass rate has to go.
It is the reason nobody knew the kernel did not boot.

### 4. Smaller items

- The font covers ASCII 32..126 plus seven supplementary glyphs. Anything else still
  falls back to `?`.
- The frame throttle in the compositor loop is a fixed 1,000-iteration spin. With the
  timer now accurate, it could be a real frame-pacing target.
- `src/apps/` renderers rebuild all layout every frame; app content is now the largest
  remaining share of frame cost, with the font rasterizer still per-pixel.
