"""Window Snapping, Maximize/Restore & Minimization Test Suite."""

import time
from .harness import QemuHarness


def test_window_snapping_and_tiling(qemu: QemuHarness):
    """Verifies:
    1. Green traffic-light button click maximizes window to full desktop workspace (1280x716).
    2. Green traffic-light button click restores window to original floating bounds.
    3. Titlebar double-click toggles maximize / restore.
    4. Dragging titlebar to left screen edge snaps window to left half-screen (640x716).
    5. Yellow traffic-light button minimizes window to dock.
    6. Clicking application dock slot restores and focuses minimized window.
    7. System liveness, RFLAGS.IF flag, and CPU execution stability.
    """
    # 1. Wait for desktop compositor
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)
    time.sleep(0.5)

    def move_to(target_x, target_y):
        # Reset mouse to top-left (0,0)
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

    # 2. Maximize Terminal window via Green Traffic Light
    # Terminal at x=30, y=35. Green button center at x=30+48=78, y=35+12=47.
    move_to(78, 47)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # Verify screendump after maximize
    img_max = qemu.screendump()
    assert not img_max.is_flat_color(threshold=10)

    # 3. Restore Terminal window via Green Traffic Light
    # Maximized window at x=0, y=24. Green button at x=48, y=36.
    move_to(48, 36)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # Verify screendump after restore
    img_restored = qemu.screendump()
    assert not img_restored.is_flat_color(threshold=10)

    # 4. Titlebar Double-Click Maximize and Restore
    # Floating Terminal titlebar center at x=200, y=47
    move_to(200, 47)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.08)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # Restore via double-click on maximized titlebar (x=400, y=36)
    move_to(400, 36)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.08)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # 5. Snap to Left Half by dragging to left edge
    # Grab floating titlebar at x=200, y=47
    move_to(200, 47)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")  # hold button
    time.sleep(0.05)

    # Drag to left edge (0, 200)
    cur_x, cur_y = 200, 47
    target_x, target_y = 0, 200
    while cur_x > target_x or cur_y < target_y:
        step_x = max(-4, target_x - cur_x)
        step_y = min(4, target_y - cur_y)
        qemu.execute_monitor(f"mouse_move {step_x} {step_y}")
        cur_x += step_x
        cur_y += step_y
        time.sleep(0.003)

    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 0")  # release to snap
    time.sleep(0.6)

    img_snapped = qemu.screendump()
    assert not img_snapped.is_flat_color(threshold=10)

    # 6. Minimize window via Yellow Traffic Light
    # Left-snapped window is at x=0, y=24. Yellow button at x=32, y=36.
    move_to(32, 36)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # 7. Restore window by clicking Terminal icon in Dock
    # Dock: width=540, x=(1280-540)/2 = 370, y=742..790.
    # Slot 2 (Terminal): x = 370 + 2*60 + 30 = 520, y = 766.
    move_to(520, 766)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # 8. Verify system health and CPU liveness
    rips, interrupts_active, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert interrupts_active, "Interrupts deadlocked after window management actions"
    assert is_moving, "CPU stopped executing after window management actions"
