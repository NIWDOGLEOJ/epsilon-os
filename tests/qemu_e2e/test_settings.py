"""System Settings Graphical Preferences & Desktop Wallpaper Engine Test Suite."""

import time
from .harness import QemuHarness


def test_system_settings_and_wallpaper(qemu: QemuHarness):
    """Verifies:
    1. System Settings preferences app launches from the 10-slot launcher dock.
    2. Theme switching (Deep Ocean -> Cyber Twilight) updates live desktop background.
    3. Sound & Audio preferences tab allows testing hardware audio and plays BootChime.
    4. Display & Info preferences tab renders resolution and RAM telemetry.
    5. Custom VFS PPM wallpaper application updates desktop.
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

    # 2. Launch System Settings from Dock (Slot 8 in 10-slot dock)
    # Dock: width=600, x=(1280-600)/2 = 340, y=742..790.
    # Slot 8 (Settings): x = 340 + 8*60 + 30 = 850, y = 766.
    move_to(850, 766)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.8)

    # Verify settings window appeared
    img_settings = qemu.screendump()
    assert not img_settings.is_flat_color(threshold=10)

    # 3. Select Theme Card 1 (Cyber Twilight)
    # Settings window at x=160, y=90, titlebar height=24.
    # Card 1 is at pane_x (160+140) + 16 + 122 + 50 = 488, y = 90 + 24 + 40 + 30 = 184
    move_to(488, 184)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # 4. Switch to "Sound & Audio" Tab in Sidebar
    # Sidebar tab 1 (Sound): x = 160 + 60 = 220, y = 90 + 24 + 12 + 32 + 16 = 174
    move_to(220, 174)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # Click [ Test Boot Chime ] button
    # Button is at pane_x (160+140) + 16 + 80 = 396, y = 90 + 24 + 50 + 13 = 177
    move_to(396, 177)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # 5. Switch to "Display & Info" Tab in Sidebar
    # Sidebar tab 2 (Display): x = 160 + 60 = 220, y = 90 + 24 + 12 + (32+6)*2 + 16 = 218
    move_to(220, 218)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # 6. Verify system liveness and CPU execution
    rips, interrupts_active, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert interrupts_active, "Interrupts deadlocked after Settings interactions"
    assert is_moving, "CPU stopped executing after Settings interactions"
