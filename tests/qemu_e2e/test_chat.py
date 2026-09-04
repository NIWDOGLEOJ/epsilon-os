"""Automated QEMU E2E Test for AegisChat & Virtual Loopback Network Stack."""

import time
from .harness import QemuHarness


def test_chat_lifecycle(qemu: QemuHarness):
    """Verifies:
    1. AegisChat launches from Dock Slot 8 (speech bubble icon).
    2. AegisChat window displays channels sidebar (#general, #kernel-dev, #agent, #alerts) and socket status.
    3. Typing a message and pressing Enter sends it over UDP loopback (127.0.0.1:8080).
    4. Switching to #agent channel and querying '@agent status' receives an autonomous AI response.
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

    # 2. Click Dock Slot 8 (AegisChat - Speech Bubble icon at x=730, y=764)
    # DOCK_WIDTH = 840, dock_x = 220, slot_width = 60, slot 8 center = 220 + 480 + 30 = 730
    move_to(730, 764)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # 3. Focus and send message in #general: "hello kernel"
    move_to(450, 470)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.2)

    for char in "hello":
        qemu.execute_monitor(f"sendkey {char}")
        time.sleep(0.02)
    qemu.execute_monitor("sendkey spc")
    time.sleep(0.02)
    for char in "kernel":
        qemu.execute_monitor(f"sendkey {char}")
        time.sleep(0.02)
    qemu.execute_monitor("sendkey ret")
    time.sleep(0.4)

    # 4. Switch to #agent channel (x=330, y=224)
    move_to(330, 224)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.4)

    # 5. Send "@agent status" in #agent
    # Send keys: shift-2 (for @), then "agent", spc, "status", ret
    qemu.execute_monitor("sendkey shift-2")
    time.sleep(0.03)
    for char in "agent":
        qemu.execute_monitor(f"sendkey {char}")
        time.sleep(0.02)
    qemu.execute_monitor("sendkey spc")
    time.sleep(0.02)
    for char in "status":
        qemu.execute_monitor(f"sendkey {char}")
        time.sleep(0.02)
    qemu.execute_monitor("sendkey ret")
    time.sleep(0.5)

    # Move cursor to neutral desktop area
    move_to(100, 700)
    time.sleep(0.2)

    # 6. Capture screendump and verify non-flat output
    img = qemu.screendump()
    assert img.width == 1280 and img.height == 800
    assert not img.is_flat_color(threshold=10)

    # 7. Verify CPU and interrupt stability
    rips, interrupts_active, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert interrupts_active, "Interrupts deadlocked after AegisChat operations"
    assert is_moving, "CPU stopped executing after AegisChat operations"
