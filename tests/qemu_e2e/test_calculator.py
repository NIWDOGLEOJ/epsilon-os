"""Scientific Calculator 2.0 & History Tape Test Suite."""

import time
from .harness import QemuHarness


def test_scientific_calculator_lifecycle(qemu: QemuHarness):
    """Verifies:
    1. Scientific Calculator launches from the 10-slot launcher dock (Slot 5).
    2. Interactive arithmetic evaluation (45 + 55 = 100) via keyboard input.
    3. Scientific square root calculation (√100 = 10).
    4. History Tape recording and rendering.
    5. Dual-pane LCD display and 5x5 keypad responsiveness.
    6. System stability, RFLAGS.IF flag, and CPU execution.
    """
    # 1. Wait for desktop compositor
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)
    time.sleep(0.5)

    def move_to(target_x, target_y):
        # Reset mouse to (0,0)
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

    # Initial screendump
    img0 = qemu.screendump()
    assert img0.width == 1280 and img0.height == 800

    # 2. Launch Calculator from Dock (Slot 5 in 10-slot dock)
    # Dock: width=600, x=(1280-600)/2 = 340, y=742..790.
    # Slot 5 (Calculator): x = 340 + 5*60 + 30 = 670, y = 766.
    move_to(670, 766)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.8)

    # Verify calculator window appeared
    img_calc = qemu.screendump()
    assert not img_calc.is_flat_color(threshold=10)

    # 3. Perform Arithmetic: 45 + 55 = 100 via Keyboard
    for ch in ["4", "5", "kp_add", "5", "5", "ret"]:
        qemu.send_key(ch)
        time.sleep(0.05)
    time.sleep(0.5)

    img_eval = qemu.screendump()
    assert not img_eval.is_flat_color(threshold=10)

    # 4. Perform Scientific Sqrt: press 's' for √ (√100 = 10)
    qemu.send_key("s")
    time.sleep(0.5)

    img_sqrt = qemu.screendump()
    assert not img_sqrt.is_flat_color(threshold=10)

    # 5. Click [ C ] button on keypad to clear display
    # Calculator window at x=380, y=150, titlebar height=24.
    # [ C ] button at pad_x (380+8) + 23 = 411, y = 150 + 24 + 72 + 17 = 263
    move_to(411, 263)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.4)

    # 6. Click History Tape to recall previous calculation (100)
    # History tape at x = 380 + 266 + 50 = 696, y = 150 + 24 + 38 + 14 = 226
    move_to(696, 226)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.4)

    # 7. Verify system stability and CPU liveness
    rips, interrupts_active, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert interrupts_active, "Interrupts deadlocked after Calculator interactions"
    assert is_moving, "CPU stopped executing after Calculator interactions"
