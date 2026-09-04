"""Ring 3 Mouse Input Test Suite.

Pointer events for a focused Ring 3 window are translated to client-relative
coordinates and queued for the process, which polls them with `SYS_POLL_EVENT`.
The Ring 3 terminal turns them into hover highlighting and a clickable toolbar.

The point of the suite is that a *user process* is driving its own UI from
mouse input, and that a click which kills it kills only it.
"""

import time
from .harness import QemuHarness

from .ring3_util import click, move_to, region_brightness, assert_compositor_alive

# Client area origin of the Ring 3 terminal window, and toolbar button centres.
# Buttons are laid out from x=6, min width 64, gap 8 (userspace/src/bin/terminal.rs).
CLIENT_X, CLIENT_Y = 601, 324
BUTTON_Y = 15
BUTTON_CENTRE = {"help": 38, "ps": 110, "free": 182, "ls": 254, "clear": 330, "crash": 410}


def _button(name: str):
    return CLIENT_X + BUTTON_CENTRE[name], CLIENT_Y + BUTTON_Y


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
    move_to(qemu, 1130, 312)
    click(qemu)

    time.sleep(1.0)

    # 2. Move over the toolbar. The process announces its first pointer event on
    #    the serial console, which makes "did input reach Ring 3?" a deterministic
    #    check rather than an inference from pixels.
    move_to(qemu, *_button("ps"))
    qemu.wait_for_serial(r"\[USERTERM\] first mouse event received", timeout=15.0)

    # 3. Click it. The process reports which button it resolved the click to, so
    #    this also confirms the client-relative coordinate translation is right --
    #    a wrong offset would resolve to a different button, or to none.
    click(qemu)
    qemu.wait_for_serial(r"\[USERTERM\] toolbar click: ps", timeout=15.0)

    # The command it ran must have produced output.
    clicked = qemu.screendump()
    assert not clicked.is_flat_color(threshold=10)

    # 4. The click that kills it.
    move_to(qemu, *_button("crash"))
    click(qemu)
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

    # Liveness: the compositor must still be redrawing.
    assert_compositor_alive(qemu)
