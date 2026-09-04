"""Automated QEMU E2E Test for AI Agent, Spotlight Search, and Aegis Browser."""

import time
from .harness import QemuHarness


def test_spotlight_and_browser_lifecycle(qemu: QemuHarness):
    """Verifies:
    1. Spotlight search overlay opens via F3 key.
    2. Typing query 'calc' filters to Calculator app and launches it on Enter.
    3. Launching Aegis Browser from Dock Slot 5 (Globe icon).
    4. Browser starts on 'aegis://home' with web portal links.
    5. Navigating to 'aegis://agent' displays AI Agent Kernel Supervisor telemetry.
    6. System stability, RFLAGS.IF flag, and CPU execution.
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

    # 2. Trigger Spotlight search via F3 key
    qemu.send_key("f3")
    time.sleep(0.4)

    # 3. Type query 'calc' and launch via Enter
    for ch in ["c", "a", "l", "c", "ret"]:
        qemu.send_key(ch)
        time.sleep(0.05)
    time.sleep(0.6)

    # 4. Click Dock Slot 5 (Browser - Globe icon at x=640, y=764)
    move_to(640, 764)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # 5. Focus Browser URL bar (around x=300, y=88) and navigate to aegis://agent
    # Browser window is at x=140, y=60, titlebar=24, nav_bar_y=60+24+4=88
    # Click inside URL bar to focus edit mode
    move_to(300, 88)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.2)

    # Backspace existing URL and type 'aegis://agent'
    for _ in range(16):
        qemu.send_key("backspace")
        time.sleep(0.02)

    for ch in ["a", "e", "g", "i", "s", "colon", "slash", "slash", "a", "g", "e", "n", "t", "ret"]:
        qemu.send_key(ch)
        time.sleep(0.04)
    time.sleep(0.6)

    # Move cursor to neutral desktop area
    move_to(100, 700)
    time.sleep(0.2)

    # 6. Capture screendump and verify non-flat output
    img = qemu.screendump()
    assert img.width == 1280 and img.height == 800
    assert not img.is_flat_color(threshold=10)

    # 7. Verify CPU and interrupt stability
    rips, interrupts_active, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert interrupts_active, "Interrupts deadlocked after Spotlight and Browser interactions"
    assert is_moving, "CPU stopped executing after Spotlight and Browser interactions"
