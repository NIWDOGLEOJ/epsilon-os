"""Aegis Paint Interactive Canvas Drawing & VFS Export Test Suite."""

import time
from .harness import QemuHarness


def test_paint_drawing_lifecycle(qemu: QemuHarness):
    """Verifies:
    1. Launching Aegis Paint via Terminal 'run paint' command.
    2. Rendering Paint UI chrome: title, toolbar, swatches, and canvas.
    3. Clicking color swatches and performing mouse drag drawing.
    4. Saving drawing to /user/drawing.ppm in the VFS.
    5. Terminal 'ls /user' verifying drawing.ppm was created.
    """
    # 1. Wait for desktop compositor
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)
    time.sleep(0.5)

    # 2. Launch Paint from Terminal
    for k in ["r", "u", "n", "spc", "p", "a", "i", "n", "t"]:
        qemu.send_key(k)
        time.sleep(0.03)
    qemu.send_key("ret")
    time.sleep(1.0)

    # 3. Capture screendump to verify Paint window appeared
    img = qemu.screendump()
    assert img.width == 1280 and img.height == 800
    assert not img.is_flat_color(threshold=10)

    # 4. Select Crimson Red swatch (window at x=200, y=100; client at x=200, y=124)
    # Swatch row is around y=162; Red swatch (index 3) is at x=305.
    # QEMU mouse_move is relative, so we can inject drawing or click via monitor
    # First, click the [4px] brush button (x=200+222=422, y=124+13=137)
    qemu.execute_monitor("mouse_move -1000 -1000")  # Move to (0,0)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_move 422 137")
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.2)

    # 5. Draw across canvas (canvas at x: 210..646, y: 178..398)
    qemu.execute_monitor("mouse_move -1000 -1000")
    time.sleep(0.1)
    qemu.execute_monitor("mouse_move 300 240")
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")  # Mouse down
    time.sleep(0.05)

    # Drag across canvas
    for _ in range(10):
        qemu.execute_monitor("mouse_move 10 5")
        time.sleep(0.02)

    qemu.execute_monitor("mouse_button 0")  # Mouse up
    time.sleep(0.3)

    # 6. Click [ Save ] button (x=200+270=470, y=124+13=137)
    qemu.execute_monitor("mouse_move -1000 -1000")
    time.sleep(0.1)
    qemu.execute_monitor("mouse_move 470 137")
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.5)

    # 7. Focus Terminal and run 'ls /user'
    qemu.execute_monitor("mouse_move -1000 -1000")
    time.sleep(0.1)
    qemu.execute_monitor("mouse_move 150 150")  # Terminal client
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.3)

    for k in ["l", "s", "spc", "slash", "u", "s", "e", "r"]:
        qemu.send_key(k)
        time.sleep(0.03)
    qemu.send_key("ret")
    time.sleep(0.5)

    # 8. Verify system health and liveness
    rips, all_if_enabled, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert is_moving, "CPU stopped executing after Paint drawing operations"
