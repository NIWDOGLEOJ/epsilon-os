"""Shared helpers for the Ring 3 test suites."""

import re
import time
from .harness import QemuHarness

# The QEMU monitor's mouse_move is relative, and the guest's PS/2 pipeline loses
# a small, constant amount of the requested travel after a reset-to-origin.
# Measured at a steady 12 px on both axes across several targets; without
# correcting for it, a click aimed at the top row of a client area lands on the
# window border instead.
CURSOR_BIAS = 12


def move_to(qemu: QemuHarness, x: int, y: int):
    """Parks the cursor at the origin, then steps to (x, y).

    Steps of 4 deliberately: the PS/2 driver's acceleration curve is 1:1 only
    below 5 counts, so a larger step travels further than it counts.
    """
    qemu.execute_monitor("mouse_move -2000 -2000")
    time.sleep(0.08)
    target_x, target_y = x + CURSOR_BIAS, y + CURSOR_BIAS
    cx = cy = 0
    while cx < target_x or cy < target_y:
        sx = min(4, target_x - cx)
        sy = min(4, target_y - cy)
        qemu.execute_monitor(f"mouse_move {sx} {sy}")
        cx += sx
        cy += sy
        time.sleep(0.003)
    time.sleep(0.3)


def click(qemu: QemuHarness):
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.06)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.8)


def wait_for_boot_settled(qemu: QemuHarness, timeout: float = 30.0):
    """Waits until the boot-time fault demo has finished.

    `main.rs` injects two deliberate faults a few compositor frames after the
    desktop comes up, and spawning and reaping a process is heavy work that runs
    with interrupts masked. Typing into the guest while that is happening loses
    keystrokes, which is what made Spotlight launches flaky: the tests were
    sleeping a fixed 1.5s and landing squarely on it.

    Waiting for the second demo fault to be reaped is deterministic, where a
    sleep is a guess.
    """
    qemu.wait_for_serial(
        r"\('crash_div_zero'\) crashed due to Divide-by-Zero", timeout=timeout
    )
    time.sleep(0.6)


def launch_ring3(qemu: QemuHarness, target: str, expect: str, attempts: int = 3):
    """Launches a Ring 3 app by typing `run <target>` into the desktop's Terminal.

    Deliberately not Spotlight. Spotlight needs a mode toggle first, and if that
    single keypress is lost every following character lands in whatever had
    focus -- measured at roughly a 75% success rate, which is not a test. The
    in-kernel Terminal is focused at boot and simply reads a line, so a dropped
    character produces a failed command and nothing else; that is the same path
    the long-standing terminal suites use, and they are reliable.

    Retried against a confirmation the guest prints. Checked before retrying,
    because launching an app that is already open only raises it and the
    program's startup lines appear once per process, so a slow-but-successful
    first attempt must not be turned into a failure.
    """
    for attempt in range(attempts):
        if re.search(expect, qemu.get_serial_log()):
            return

        for ch in f"run {target}":
            qemu.send_key("spc" if ch == " " else ch)
            time.sleep(0.05)
        qemu.send_key("ret")
        time.sleep(1.2)

        try:
            qemu.wait_for_serial(expect, timeout=8.0)
            return
        except Exception:
            if attempt == attempts - 1:
                raise


def click_until_serial(qemu: QemuHarness, x: int, y: int, expect: str, key: str = None,
                       attempts: int = 3):
    """Clicks at (x, y) until the guest confirms it on the serial console.

    A single absolute-positioned click is not reliable under load: the cursor is
    driven by relative PS/2 packets, and a missed one puts the pointer somewhere
    else entirely. Retrying against a confirmation the guest prints turns that
    from a flaky test into a slow one.
    """
    for attempt in range(attempts):
        move_to(qemu, x, y)
        click(qemu)
        if key is not None:
            qemu.send_key(key)
        try:
            qemu.wait_for_serial(expect, timeout=6.0)
            return
        except Exception:
            if attempt == attempts - 1:
                raise


def region_brightness(img, x: int, y: int, w: int, h: int) -> int:
    """Sums channel values over a region, sampling every other pixel.

    Compared against the same region in another frame, this detects a change
    without depending on exact colours. Deliberately scoped to a region rather
    than the whole screen, so a comparison says something about the thing under
    test rather than about the menu bar clock.
    """
    total = 0
    for yy in range(y, y + h, 2):
        for xx in range(x, x + w, 2):
            r, g, b = img.get_pixel(xx, yy)
            total += r + g + b
    return total


def assert_compositor_alive(qemu: QemuHarness, timeout: float = 8.0):
    """Waits for the desktop to visibly redraw.

    The menu bar carries a wall-clock uptime the kernel redraws every frame, so
    the strip changing proves the compositor is still running. Polled rather
    than sampled once: under TCG the guest can take several seconds to advance a
    second, and a single comparison across a fixed gap turns that into a flake.

    Sampling RIP instead would report a healthy kernel idling in `hlt` as a
    stopped one, which is why this exists at all.
    """
    first = qemu.screendump()
    assert not first.is_flat_color(threshold=10), "Desktop stopped rendering"
    baseline = region_brightness(first, 0, 0, 1280, 24)

    deadline = time.time() + timeout
    while time.time() < deadline:
        time.sleep(1.0)
        current = qemu.screendump()
        if region_brightness(current, 0, 0, 1280, 24) != baseline:
            return
    raise AssertionError(
        f"Menu bar did not change in {timeout}s; the compositor is no longer running"
    )
