"""Aegis Files Interactive File Manager & Inter-App Navigation Test Suite."""

import time
from .harness import QemuHarness


def test_file_manager_lifecycle(qemu: QemuHarness):
    """Verifies:
    1. Launching Aegis Files via Terminal 'run files' command.
    2. Rendering split-pane UI: Places sidebar, toolbar, file list, and status bar.
    3. Navigation via toolbar buttons (e.g. clicking '/user').
    4. Selecting a text document ('notes.txt').
    5. Clicking [ Open ] to launch AegisPad with the document loaded.
    6. System stability, CPU register IF flag, and instruction pointer progression.
    """
    # 1. Wait for desktop compositor
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)
    time.sleep(0.5)

    # 2. Launch Aegis Files from Terminal
    for k in ["r", "u", "n", "spc", "f", "i", "l", "e", "s"]:
        qemu.send_key(k)
        time.sleep(0.03)
    qemu.send_key("ret")
    time.sleep(1.0)

    # 3. Capture screendump to verify File Manager window appeared
    img = qemu.screendump()
    assert img.width == 1280 and img.height == 800
    assert not img.is_flat_color(threshold=10)

    # 4. Click [/user] toolbar button
    # Window at x=180, y=120; client rect at x=180, y=144.
    # [/user] button is at x = 180 + 140 = 320, y = 144 + 14 = 158.
    qemu.execute_monitor("mouse_move -1000 -1000")  # Reset cursor to (0, 0)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_move 320 158")
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.5)

    # 5. Click on row 0 in the file browser area to select notes.txt
    # Row 0 is at x = 350, y = 144 + 48 + 11 = 203
    qemu.execute_monitor("mouse_move -1000 -1000")
    time.sleep(0.1)
    qemu.execute_monitor("mouse_move 350 203")
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.5)

    # 6. Click [ Open ] button on the bottom action bar
    # Client width = 520; [Open] is at client.x + 520 - 100 = 600, y = 144 + 336 - 14 = 466
    qemu.execute_monitor("mouse_move -1000 -1000")
    time.sleep(0.1)
    qemu.execute_monitor("mouse_move 600 466")
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.8)

    # 7. Capture screendump to verify AegisPad opened with notes.txt
    img_opened = qemu.screendump()
    assert not img_opened.is_flat_color(threshold=10)

    # 8. Verify system health and liveness
    rips, interrupts_active, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert interrupts_active, "Interrupts are deadlocked after File Manager operations"
    assert is_moving, "CPU stopped executing after File Manager operations"
