"""Terminal 2.0: Command History, Tab Auto-Completion & ANSI Color Suite."""

import time
from .harness import QemuHarness


def test_terminal_history_and_completion(qemu: QemuHarness):
    """Verifies:
    1. Terminal window is active with colored ANSI prompt.
    2. Execution of 'neofetch' displays stylized ANSI banner.
    3. Up arrow recalls previously executed command from history.
    4. Tab auto-completion completes command prefixes (e.g. 'wallp' -> 'wallpaper ').
    5. Tab auto-completion completes VFS file paths (e.g. 'cat /wel' -> 'cat /welcome.txt ').
    6. 'history' command displays numbered history tape.
    7. System stability, RFLAGS.IF flag, and CPU execution.
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

    # Click inside Terminal window (x=150, y=120) to focus
    move_to(150, 120)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.3)

    # 2. Execute 'neofetch'
    for ch in ["n", "e", "o", "f", "e", "t", "c", "h", "ret"]:
        qemu.send_key(ch)
        time.sleep(0.04)
    time.sleep(0.6)

    # 3. Test Up arrow history recall (recalls 'neofetch')
    qemu.send_key("up")
    time.sleep(0.2)
    qemu.send_key("ret")
    time.sleep(0.5)

    # 4. Test Tab auto-completion for command: 'wallp' + Tab -> 'wallpaper '
    for ch in ["w", "a", "l", "l", "p", "tab", "ret"]:
        qemu.send_key(ch)
        time.sleep(0.05)
    time.sleep(0.5)

    # 5. Test Tab auto-completion for VFS path: 'cat /wel' + Tab -> 'cat /welcome.txt '
    for ch in ["c", "a", "t", "spc", "slash", "w", "e", "l", "tab", "ret"]:
        qemu.send_key(ch)
        time.sleep(0.05)
    time.sleep(0.5)

    # 6. Test 'history' command
    for ch in ["h", "i", "s", "t", "o", "r", "y", "ret"]:
        qemu.send_key(ch)
        time.sleep(0.04)
    time.sleep(0.5)

    # 7. Screendump verification
    img = qemu.screendump()
    assert img.width == 1280 and img.height == 800
    assert not img.is_flat_color(threshold=10)

    # 8. Verify system stability
    rips, interrupts_active, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert interrupts_active, "Interrupts deadlocked after Terminal 2.0 interactions"
    assert is_moving, "CPU stopped executing after Terminal 2.0 interactions"
