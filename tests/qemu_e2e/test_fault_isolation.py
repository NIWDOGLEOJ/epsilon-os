"""Ring 3 Hardware Fault Isolation and Crash Resilience Test Suite for AegisOS."""

import time
from .harness import QemuHarness


def test_boot_fault_isolation(qemu: QemuHarness):
    """Verifies that early boot Ring 3 self-test faults (#PF and #DE) are trapped
    and isolated by hardware without halting the kernel or freezing the desktop.
    """
    # 1. Page Fault (#PF, vector 14) at RIP 0x400000, CR2 0x0
    qemu.wait_for_serial(
        r"\[FAULT-ISOLATION\] Userspace Fault caught: Page Fault \(#PF\) \(vec 14\) at RIP=0x0000000000400000",
        timeout=12.0,
    )
    qemu.wait_for_serial(
        r"\[FAULT-ISOLATION\] Process PID \d+ \('crash_null_ptr'\) crashed due to Page Fault \(#PF\)"
    )
    qemu.wait_for_serial(
        r"\[FAULT-TELEMETRY\] Ring 3 Task PID \d+ caught Page Fault \(#PF\) at RIP 0x0000000000400000 \(CR2: 0x0000000000000000\)\. Desktop remains 100% stable\."
    )

    # 2. Divide-by-Zero (#DE, vector 0) at RIP 0x400007
    qemu.wait_for_serial(
        r"\[FAULT-ISOLATION\] Userspace Fault caught: Divide-by-Zero \(#DE\) \(vec 0\) at RIP=0x0000000000400007",
        timeout=8.0,
    )
    qemu.wait_for_serial(
        r"\[FAULT-ISOLATION\] Process PID \d+ \('crash_div_zero'\) crashed due to Divide-by-Zero \(#DE\)"
    )
    qemu.wait_for_serial(
        r"\[FAULT-TELEMETRY\] Ring 3 Task PID \d+ caught Divide-by-Zero \(#DE\) at RIP 0x0000000000400007 \(CR2: 0x0000000000000000\)\. Desktop remains 100% stable\."
    )


def test_crashtest_all_buttons(qemu: QemuHarness):
    """Interactively clicks all four buttons in the Crash-Test application
    and verifies that each hardware fault vector is correctly trapped, logged,
    and isolated while the desktop environment remains running.
    """
    # Ensure compositor is up
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)
    time.sleep(1.0)

    # --- Button #0: Null Pointer Dereference (#PF) ---
    qemu.mouse_move(50, 35)
    time.sleep(0.1)
    qemu.mouse_click(button=1)
    qemu.wait_for_serial(r"\[CRASH-TEST\] User clicked fault button #0\.", timeout=5.0)
    qemu.wait_for_serial(r"\[FAULT-ISOLATION\] Userspace Fault caught: Page Fault \(#PF\) \(vec 14\)")
    qemu.wait_for_serial(r"\[FAULT-TELEMETRY\] Ring 3 Task PID \d+ caught Page Fault \(#PF\).*Desktop remains 100% stable\.")

    # --- Button #1: Divide by Zero (#DE) ---
    qemu.mouse_move(0, 24)
    time.sleep(0.1)
    qemu.mouse_click(button=1)
    qemu.wait_for_serial(r"\[CRASH-TEST\] User clicked fault button #1\.", timeout=5.0)
    qemu.wait_for_serial(r"\[FAULT-ISOLATION\] Userspace Fault caught: Divide-by-Zero \(#DE\) \(vec 0\)")
    qemu.wait_for_serial(r"\[FAULT-TELEMETRY\] Ring 3 Task PID \d+ caught Divide-by-Zero \(#DE\).*Desktop remains 100% stable\.")

    # --- Button #2: Out-of-Bounds Supervisor Write (#PF) ---
    qemu.mouse_move(0, 24)
    time.sleep(0.1)
    qemu.mouse_click(button=1)
    qemu.wait_for_serial(r"\[CRASH-TEST\] User clicked fault button #2\.", timeout=5.0)
    qemu.wait_for_serial(r"\[FAULT-ISOLATION\] Userspace Fault caught: Page Fault \(#PF\) \(vec 14\) .* CR2=0xffffffff80000000")
    qemu.wait_for_serial(r"\[FAULT-TELEMETRY\] Ring 3 Task PID \d+ caught Page Fault \(#PF\).*Desktop remains 100% stable\.")

    # --- Button #3: Invalid Opcode (#UD) ---
    qemu.mouse_move(0, 24)
    time.sleep(0.1)
    qemu.mouse_click(button=1)
    qemu.wait_for_serial(r"\[CRASH-TEST\] User clicked fault button #3\.", timeout=5.0)
    qemu.wait_for_serial(r"\[FAULT-ISOLATION\] Userspace Fault caught: Invalid Opcode \(#UD\) \(vec 6\)")
    qemu.wait_for_serial(r"\[FAULT-TELEMETRY\] Ring 3 Task PID \d+ caught Invalid Opcode \(#UD\).*Desktop remains 100% stable\.")
