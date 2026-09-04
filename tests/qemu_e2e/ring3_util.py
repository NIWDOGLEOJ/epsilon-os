"""Shared helpers for the Ring 3 test suites."""

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
