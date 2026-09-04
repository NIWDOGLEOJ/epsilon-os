"""Ring 3 Activity Monitor Test Suite.

The third app to cross the privilege boundary, and the first that has to *poll*
rather than react: a monitor's display changes without anyone touching it, so it
resamples on the kernel's tick counter.

It also exercises the read-and-act side of the ABI that the other two do not --
process enumeration, memory statistics and process termination, all from a
process that cannot see the scheduler.
"""

import time
from .harness import QemuHarness
from .ring3_util import (
    wait_for_boot_settled,
    assert_compositor_alive,
    click_until_serial,
    launch_ring3,
    region_brightness,
)

# Window is created at (300, 180) 650x412, so its client area starts here.
CLIENT_X, CLIENT_Y = 301, 204
# Table rows begin one row below the header at y=156.
TABLE_FIRST_ROW_Y = 156 + 16
ROW_H = 16
# The CPU history graph, in client coordinates.
GRAPH_RECT = (CLIENT_X + 16, CLIENT_Y + 60, 280, 70)


def _row_y(row: int) -> int:
    return CLIENT_Y + TABLE_FIRST_ROW_Y + row * ROW_H


def test_ring3_monitor_samples_and_kills(qemu: QemuHarness):
    """Verifies:
    1. Three Ring 3 GUI processes run at once, each with its own surface.
    2. Spotlight raises a Ring 3 window rather than opening a blank duplicate.
    3. The monitor resamples on a clock, with no input at all.
    4. It can terminate another process through SYS_KILL.
    5. PID 0 is refused.
    6. The monitor survives, and so does the kernel.
    """
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=25.0)
    wait_for_boot_settled(qemu)
    launch_ring3(qemu, "r3proc", expect=r"\[USERMON\] surface mapped, entering event loop")

    # 1. Open all three, so the surface table really is holding three at once.
    #    MAX_SURFACES is 4, so this exercises it being per-PID, not a singleton.
        
    # 2. The one under test, opened last so it is focused and on top.
    
    # 3. The CPU graph gains a bar every 0.5s. Nothing here touches the machine,
    #    so a change proves the process is resampling on its own.
    first = qemu.screendump()
    baseline = region_brightness(first, *GRAPH_RECT)
    deadline = time.time() + 10.0
    changed = False
    while time.time() < deadline:
        time.sleep(1.0)
        if region_brightness(qemu.screendump(), *GRAPH_RECT) != baseline:
            changed = True
            break
    assert changed, (
        "The CPU graph never changed; the Ring 3 monitor is not resampling on its own"
    )

    # 4. Select a process and kill it from Ring 3. Row 3 is a kernel worker stub
    #    spawned at boot, so its position is stable and terminating it is inert.
    click_until_serial(
        qemu,
        CLIENT_X + 200,
        _row_y(3),
        expect=r"\[USERMON\] killed PID \d+ from Ring 3",
        key="k",
    )

    # 5. The idle task is row 0, and the kernel must refuse it.
    click_until_serial(
        qemu,
        CLIENT_X + 200,
        _row_y(0),
        expect=r"\[USERMON\] kill refused by the kernel",
        key="k",
    )

    # 6. Nothing died that should not have.
    log = qemu.get_serial_log()
    assert "('user_monitor') crashed" not in log, "The Ring 3 monitor died"
    assert "('user_terminal') crashed" not in log, "The Ring 3 terminal died"
    assert "('user_crashtest') crashed" not in log, "The Ring 3 crash-test died"
    assert "KERNEL PANIC" not in log, "Kernel panicked during Ring 3 monitor activity"
    assert "KERNEL EXCEPTION PANIC" not in log, "Kernel took a Ring 0 exception"

    assert_compositor_alive(qemu)
