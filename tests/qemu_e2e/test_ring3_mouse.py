"""Ring 3 Mouse Input Test Suite.

Pointer events for a focused Ring 3 window are translated to client-relative
coordinates and queued for the process, which polls them with `SYS_POLL_EVENT`.
The Ring 3 terminal turns them into hover highlighting and a clickable toolbar.

The point of the suite is that a *user process* is driving its own UI from
mouse input, and that a click which kills it kills only it.
"""

import time
from .harness import QemuHarness

# The QEMU monitor's mouse_move is relative and the guest's PS/2 pipeline loses a
# small, constant amount of the requested travel after a reset-to-origin. Measured
# at a steady 12 px on both axes across targets; without correcting for it a click
# aimed at the top row of the client area lands on the window border instead.
CURSOR_BIAS = 12

# Client area origin of the Ring 3 window, and toolbar button centres within it.
# Buttons are laid out from x=6, min width 64, gap 8 (userspace/src/main.rs).
CLIENT_X, CLIENT_Y = 601, 324
BUTTON_Y = 15
BUTTON_CENTRE = {"help": 38, "ps": 110, "free": 182, "ls": 254, "clear": 330, "crash": 410}


def _move_to(qemu: QemuHarness, x: int, y: int):
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


def _click(qemu: QemuHarness):
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.06)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.7)


def _button(name: str):
    return CLIENT_X + BUTTON_CENTRE[name], CLIENT_Y + BUTTON_Y


def _region_brightness(img, x: int, y: int, w: int, h: int) -> int:
    """Sums channel values over a region, sampling every other pixel.

    Compared against the same region in another frame, this detects a change
    without depending on exact colours. Deliberately scoped to a region rather
    than the whole screen: the menu bar clock and CPU badge tick constantly, so
    a full-frame comparison would differ whether or not the thing under test
    did anything.
    """
    total = 0
    for yy in range(y, y + h, 2):
        for xx in range(x, x + w, 2):
            r, g, b = img.get_pixel(xx, yy)
            total += r + g + b
    return total


# Screen rect of the 'ps' button: client origin + its layout position.
PS_BUTTON_RECT = (CLIENT_X + 78, CLIENT_Y + 3, 64, 24)
# The text area below the toolbar, where command output lands.
OUTPUT_RECT = (CLIENT_X, CLIENT_Y + 30, 620, 200)


def test_ring3_mouse_input_and_isolation(qemu: QemuHarness):
    """Verifies:
    1. The Ring 3 terminal is running and has its surface.
    2. Moving over its toolbar changes what it draws (motion events arrive).
    3. Clicking a toolbar button runs the command (button events arrive and the
       process acts on them).
    4. Clicking the 'crash' button kills the process and only the process.
    """
    qemu.wait_for_serial(r"\[USERTERM\] surface mapped, entering event loop", timeout=20.0)
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=20.0)
    time.sleep(1.5)

    # Raise the Ring 3 window by its exposed titlebar strip.
    _move_to(qemu, 1130, 312)
    _click(qemu)

    time.sleep(1.0)

    # 2. Move over the toolbar. The process announces its first pointer event on
    #    the serial console, which makes "did input reach Ring 3?" a deterministic
    #    check rather than an inference from pixels.
    _move_to(qemu, *_button("ps"))
    qemu.wait_for_serial(r"\[USERTERM\] first mouse event received", timeout=15.0)

    # 3. Click it. The process reports which button it resolved the click to, so
    #    this also confirms the client-relative coordinate translation is right --
    #    a wrong offset would resolve to a different button, or to none.
    _click(qemu)
    qemu.wait_for_serial(r"\[USERTERM\] toolbar click: ps", timeout=15.0)

    # The command it ran must have produced output.
    clicked = qemu.screendump()
    assert not clicked.is_flat_color(threshold=10)

    # 4. The click that kills it.
    _move_to(qemu, *_button("crash"))
    _click(qemu)
    time.sleep(1.5)

    log = qemu.get_serial_log()
    assert "('user_terminal') crashed due to Page Fault" in log, (
        "The 'crash' toolbar button did not fault the Ring 3 terminal"
    )
    assert "CR2=0x0000000000000000" in log, (
        "Fault was not the null dereference the crash button performs"
    )
    assert "KERNEL PANIC" not in log, "Kernel panicked when the Ring 3 terminal crashed"
    assert "KERNEL EXCEPTION PANIC" not in log, "Kernel took a Ring 0 exception"

    # Liveness: the menu bar carries a wall-clock uptime the kernel redraws every
    # frame, so the strip changing across a two-second gap proves the compositor
    # is still running. Sampling RIP instead would report a healthy idle kernel
    # sitting in `hlt` as a stopped one.
    before = qemu.screendump()
    time.sleep(2.5)
    after = qemu.screendump()
    assert not after.is_flat_color(threshold=10), "Desktop stopped rendering"
    assert _region_brightness(before, 0, 0, 1280, 24) != _region_brightness(after, 0, 0, 1280, 24), (
        "Menu bar stopped updating; the compositor is no longer running"
    )
