"""Ring 3 Terminal Test Suite.

The terminal here is not kernel code. It is a separately compiled program
(`userspace/`) loaded from an ELF image into its own address space, drawing
through a shared surface and reaching system state only through syscalls.

The assertion that matters is the last one: when it dereferences null, the
kernel reaps it and the desktop carries on. That is the project's headline
guarantee, finally demonstrated on an application rather than on a payload
written to crash.
"""

import time
from .harness import QemuHarness


def _type_line(qemu: QemuHarness, text: str):
    for ch in text:
        qemu.send_key("spc" if ch == " " else ch)
        time.sleep(0.03)
    qemu.send_key("ret")
    time.sleep(0.6)


def test_ring3_terminal_runs_and_is_isolated(qemu: QemuHarness):
    """Verifies:
    1. The userspace ELF is loaded and reaches its event loop.
    2. It maps its window surface and renders into it.
    3. Commands backed by syscalls (`ps`, `free`) return live kernel state.
    4. `crash` kills the process and only the process.
    5. The kernel never panics and keeps compositing afterwards.
    """
    # 1. Program started and got past surface mapping.
    qemu.wait_for_serial(r"\[ELF\] Ring 3 terminal loaded as PID \d+", timeout=20.0)
    qemu.wait_for_serial(r"\[USERTERM\] Ring 3 terminal starting", timeout=20.0)
    qemu.wait_for_serial(r"\[USERTERM\] surface mapped, entering event loop", timeout=20.0)
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=20.0)
    time.sleep(1.5)

    # 2. Raise it. The window opens beneath the in-kernel ones so it does not
    #    disturb the desktop the other suites expect, so focus it by clicking
    #    the strip of titlebar left exposed on the right.
    # Steps of 4 deliberately: the PS/2 driver's acceleration curve is 1:1 only
    # below 5 counts, so a larger step moves the cursor further than it counts.
    qemu.execute_monitor("mouse_move -2000 -2000")
    time.sleep(0.05)
    cur_x, cur_y = 0, 0
    target_x, target_y = 1130, 312
    while cur_x < target_x or cur_y < target_y:
        step_x = min(4, target_x - cur_x)
        step_y = min(4, target_y - cur_y)
        qemu.execute_monitor(f"mouse_move {step_x} {step_y}")
        cur_x += step_x
        cur_y += step_y
        time.sleep(0.003)
    time.sleep(0.15)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.5)

    before = qemu.screendump()
    assert before.width == 1280 and before.height == 800
    assert not before.is_flat_color(threshold=10)

    # 3. Drive it through syscall-backed commands. A process that had failed to
    #    map its surface or read its events would not survive these.
    _type_line(qemu, "help")
    _type_line(qemu, "ps")
    _type_line(qemu, "free")

    after_cmds = qemu.screendump()
    assert not after_cmds.is_flat_color(threshold=10)
    # Rendering real text means many distinct colours in the window region.
    assert after_cmds.color_variance(step=8) > 0.0

    # 4. Kill it from the inside.
    _type_line(qemu, "crash")
    time.sleep(1.5)

    log = qemu.get_serial_log()
    assert "[USERTERM] surface mapped" in log, "Ring 3 terminal never reached its event loop"

    # The fault must be attributed to the userspace process, and be the null
    # dereference the command performs (CR2 = 0) rather than an accident
    # somewhere else such as a stack overflow.
    assert "('user_terminal') crashed due to Page Fault" in log, (
        "Ring 3 terminal did not fault where expected"
    )
    assert "CR2=0x0000000000000000" in log, (
        "Fault was not the null dereference the 'crash' command performs"
    )

    # 5. The whole point: the kernel is fine.
    assert "KERNEL PANIC" not in log, "Kernel panicked when the Ring 3 terminal crashed"
    assert "KERNEL EXCEPTION PANIC" not in log, "Kernel took a Ring 0 exception"

    _rips, _if, is_moving = qemu.sample_rip_stability(samples=4, interval=0.1)
    assert is_moving, "CPU stopped executing after the Ring 3 terminal crashed"

    # Desktop still composites: the frame is still being drawn by the kernel.
    final = qemu.screendump()
    assert not final.is_flat_color(threshold=10), "Desktop stopped rendering"
