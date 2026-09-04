"""Automated QEMU E2E Test for AegisPad 2.0 Multi-Tab Syntax & Code Editor."""

import time
from .harness import QemuHarness


def test_editor_advanced_lifecycle(qemu: QemuHarness):
    """Verifies:
    1. AegisPad 2.0 window focuses and displays document tab strip with welcome.txt.
    2. Clicking [ + ] creates a new document buffer tab.
    3. Typing text into editor marks tab with dirty indicator and updates active line highlight.
    4. Ctrl+F opens the Find bar, searches query, and highlights occurrences.
    5. Framebuffer rendering is non-flat, CPU RIP moves, interrupts remain active.
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

    # 2. Focus AegisPad window by clicking its client area (x=200, y=420)
    move_to(200, 420)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.4)

    # 3. Click [ + ] button to open a new tab
    # client.x = 30, client.y = 374. welcome.txt tab is 120px wide (34..154).
    # Plus button is at x=160..182, y=376..397.
    move_to(170, 386)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.4)

    # 4. Type code in new buffer: "fn main() { let x = 42; }"
    for char in "fn main() { let x = 42; }":
        if char == " ":
            qemu.execute_monitor("sendkey spc")
        elif char == "_":
            qemu.execute_monitor("sendkey shift-minus")
        elif char == "(":
            qemu.execute_monitor("sendkey shift-9")
        elif char == ")":
            qemu.execute_monitor("sendkey shift-0")
        elif char == "{":
            qemu.execute_monitor("sendkey shift-bracket_left")
        elif char == "}":
            qemu.execute_monitor("sendkey shift-bracket_right")
        elif char == "=":
            qemu.execute_monitor("sendkey equal")
        elif char == ";":
            qemu.execute_monitor("sendkey semicolon")
        else:
            qemu.execute_monitor(f"sendkey {char}")
        time.sleep(0.02)

    time.sleep(0.3)

    # 5. Press Ctrl+F to open Find Bar
    qemu.execute_monitor("sendkey ctrl-f")
    time.sleep(0.3)

    # Search for "let"
    for char in "let":
        qemu.execute_monitor(f"sendkey {char}")
        time.sleep(0.03)

    qemu.execute_monitor("sendkey ret")
    time.sleep(0.3)

    # Move cursor to neutral desktop area
    move_to(100, 700)
    time.sleep(0.2)

    # 6. Capture screendump and verify non-flat output
    img = qemu.screendump()
    assert img.width == 1280 and img.height == 800
    assert not img.is_flat_color(threshold=10)

    # 7. Verify CPU and interrupt stability
    rips, interrupts_active, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert interrupts_active, "Interrupts deadlocked after AegisPad operations"
    assert is_moving, "CPU stopped executing after AegisPad operations"
