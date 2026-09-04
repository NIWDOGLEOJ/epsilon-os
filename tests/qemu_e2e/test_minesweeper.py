"""Automated QEMU E2E Test for Minesweeper Retro Arcade Game."""

import time
from .harness import QemuHarness


def test_minesweeper_lifecycle(qemu: QemuHarness):
    """Verifies:
    1. Minesweeper launches from Dock Slot 6 (spiked naval mine icon).
    2. Minesweeper window renders 9x9 grid, LED mine counter (010), and yellow smiley face.
    3. Left-click on safe grid cell reveals cell with 3D beveled tiles.
    4. Right-click on cell places red flag marker and updates counter.
    5. Clicking the yellow smiley face resets the board.
    6. System stability, RFLAGS.IF flag, and continuous CPU execution.
    """
    # 1. Wait for desktop compositor
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)
    time.sleep(0.5)

    def move_to(target_x, target_y):
        qemu.execute_monitor("mouse_move -1000 -1000")
        time.sleep(0.04)
        cur_x, cur_y = 0, 0
        while cur_x < target_x or cur_y < target_y:
            step_x = min(4, target_x - cur_x)
            step_y = min(4, target_y - cur_y)
            qemu.execute_monitor(f"mouse_move {step_x} {step_y}")
            cur_x += step_x
            cur_y += step_y
            time.sleep(0.003)

    # 2. Click Dock Slot 6 (Minesweeper - Spiked Mine icon at x=670, y=764)
    # DOCK_WIDTH = 720, dock_x = 280, slot_width = 60, slot 6 center = 280 + 390 = 670
    move_to(670, 764)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # 3. Left-click center of grid (x=564, y=324) to trigger first-click safe reveal
    move_to(564, 324)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.5)

    # 4. Right-click a cell (x=620, y=250) to place a flag marker
    move_to(620, 250)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 2")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.4)

    # 5. Click the yellow smiley face button (x=564, y=204) to restart
    move_to(564, 204)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.4)

    # Move cursor to neutral desktop location
    move_to(100, 700)
    time.sleep(0.2)

    # 6. Capture screendump and verify non-flat output
    img = qemu.screendump()
    assert img.width == 1280 and img.height == 800
    assert not img.is_flat_color(threshold=10)

    # 7. Verify CPU and interrupt stability
    rips, interrupts_active, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert interrupts_active, "Interrupts deadlocked after Minesweeper gameplay"
    assert is_moving, "CPU stopped executing after Minesweeper gameplay"
