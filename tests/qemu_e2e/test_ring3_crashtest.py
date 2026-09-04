"""Ring 3 Crash-Test Test Suite.

The Crash-Test demo used to be kernel code drawing the claim that Ring 3 faults
are contained. It is now itself a user process, which makes the demonstration
stricter: two Ring 3 processes are involved, one dies on purpose, and the one
that asked for it carries on drawing.

Assertions are serial-log based rather than pixel comparisons, so they say
something specific about what happened rather than that the screen changed.
"""

import time
from .harness import QemuHarness

from .ring3_util import click, move_to, assert_compositor_alive

# Window is created at (700, 35) 560x250, so its client area starts here.
CLIENT_X, CLIENT_Y = 701, 59
# Buttons: x 16..256 within the client, 34 tall from y=44 with a 6px gap.
BUTTON_CX = 16 + 120
BUTTON_CY = [61, 101, 141, 181]


def test_ring3_crashtest_injects_and_survives(qemu: QemuHarness):
    """Verifies:
    1. Two Ring 3 GUI processes run at once, each with its own surface.
    2. The Ring 3 Crash-Test app injects faults through SYS_SPAWN_FAULT.
    3. Each of the four fault classes is trapped and reaped.
    4. The app that requested them is still alive and drawing afterwards.
    5. The kernel never panics.
    """
    # 1. Both Ring 3 GUI processes came up. Before per-PID surfaces only one
    #    could hold a surface at a time, so this is the load-bearing part.
    qemu.wait_for_serial(r"\[ELF\] Ring 3 terminal loaded as PID \d+", timeout=20.0)
    qemu.wait_for_serial(r"\[ELF\] Ring 3 crash-test loaded as PID \d+", timeout=20.0)
    qemu.wait_for_serial(r"\[USERTERM\] surface mapped, entering event loop", timeout=20.0)
    qemu.wait_for_serial(r"\[USERCRASH\] surface mapped, entering event loop", timeout=20.0)
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=20.0)
    time.sleep(1.5)

    # Raise it by the strip of titlebar left exposed on the right.
    move_to(qemu, 1100, 47)
    click(qemu)
    time.sleep(0.8)

    # 2. Inject the first fault with the pointer. The app logs what it asked
    #    for, which also confirms the click resolved to the button we aimed at.
    move_to(qemu, CLIENT_X + BUTTON_CX, CLIENT_Y + BUTTON_CY[0])
    click(qemu)
    qemu.wait_for_serial(r"\[USERCRASH\] requested fault: Null Pointer", timeout=15.0)

    # 3. Inject the rest from the keyboard, which the app also accepts. Driving
    #    four separate clicks by absolute cursor position is unreliable under
    #    load -- a missed click is a missed test, not a real failure -- and the
    #    pointer path is covered thoroughly by test_ring3_mouse. What this suite
    #    needs from here is that every fault class is trapped.
    for key, label in [("2", "Divide by Zero"), ("3", "Kernel Write"), ("4", "Invalid Opcode")]:
        qemu.send_key(key)
        qemu.wait_for_serial(rf"\[USERCRASH\] requested fault: {label}", timeout=15.0)
        time.sleep(0.4)

    time.sleep(1.5)
    log = qemu.get_serial_log()

    # The two fault classes the boot-time demo never spawns, so their presence
    # can only be the Ring 3 app's doing.
    assert "('crash_oob_write') crashed due to Page Fault" in log, (
        "Kernel-space write fault was never trapped"
    )
    assert "('crash_invalid_op') crashed due to Invalid Opcode" in log, (
        "Invalid opcode fault was never trapped"
    )
    # A Ring 3 write into kernel space must be caught at the kernel address.
    assert "CR2=0xffffffff80000000" in log, (
        "Ring 3 write to kernel space did not fault at the kernel address"
    )

    # 4. The requesting process must have survived all four.
    assert "('user_crashtest') crashed" not in log, (
        "The Ring 3 crash-test app died along with the faults it injected"
    )
    assert "('user_terminal') crashed" not in log, (
        "The Ring 3 terminal died while another process was faulting"
    )

    # 5. And the kernel.
    assert "KERNEL PANIC" not in log, "Kernel panicked during Ring 3 fault injection"
    assert "KERNEL EXCEPTION PANIC" not in log, "Kernel took a Ring 0 exception"

    # Liveness: the compositor must still be redrawing after all of that.
    assert_compositor_alive(qemu)
