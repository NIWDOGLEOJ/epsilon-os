"""Interactive Terminal Shell Application Test Suite for AegisOS."""

import time
from .harness import QemuHarness


def test_terminal_shell_interaction(qemu: QemuHarness):
    """Verifies that the interactive Terminal shell responds to keyboard input,
    executes shell commands, and supports userspace fault injection via the CLI.
    """
    # Wait for desktop compositor to initialize
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)
    time.sleep(1.0)

    # By default, Terminal window is focused at boot.
    # 1. Type 'help' command
    qemu.send_string("help\n", delay_between_keys=0.04)
    time.sleep(0.5)

    # 2. Type 'calc 16 * 4' command
    qemu.send_string("calc 16 * 4\n", delay_between_keys=0.04)
    time.sleep(0.5)

    # 3. Inject fault via terminal CLI: 'crash 1' (Divide by Zero)
    qemu.send_string("crash 1\n", delay_between_keys=0.04)
    qemu.wait_for_serial(r"\[FAULT-ISOLATION\] Userspace Fault caught: Divide-by-Zero \(#DE\) \(vec 0\)", timeout=5.0)
    qemu.wait_for_serial(r"\[FAULT-TELEMETRY\] Ring 3 Task PID \d+ caught Divide-by-Zero \(#DE\).*Desktop remains 100% stable\.")

    # 4. Inject fault via terminal CLI: 'crash 3' (Invalid Opcode ud2)
    qemu.send_string("crash 3\n", delay_between_keys=0.04)
    qemu.wait_for_serial(r"\[FAULT-ISOLATION\] Userspace Fault caught: Invalid Opcode \(#UD\) \(vec 6\)", timeout=5.0)
    qemu.wait_for_serial(r"\[FAULT-TELEMETRY\] Ring 3 Task PID \d+ caught Invalid Opcode \(#UD\).*Desktop remains 100% stable\.")

    # 5. Capture screenshot and verify terminal window content rendered
    img = qemu.screendump()
    # Terminal client rect is around x in 31..460, y in 60..330
    terminal_sample = img.get_pixel(100, 100)
    # Terminal background is dark (~Color::rgb(20, 22, 26))
    assert terminal_sample[0] < 50 and terminal_sample[1] < 50 and terminal_sample[2] < 50, (
        f"Unexpected terminal background pixel: {terminal_sample}"
    )
