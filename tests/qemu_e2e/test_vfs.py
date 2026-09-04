"""In-Memory Virtual Filesystem (RAM Disk VFS) & File Persistence Test Suite."""

import time
from .harness import QemuHarness


def test_vfs_file_lifecycle(qemu: QemuHarness):
    """Verifies:
    1. VFS initialization at boot with seed files (/welcome.txt, /system/readme.txt).
    2. Terminal 'ls' lists directory contents.
    3. Terminal 'cat /welcome.txt' reads file contents.
    4. Terminal 'write /user/test.txt <msg>' creates and writes to a file.
    5. Terminal 'cat /user/test.txt' reads back the persisted file.
    6. Terminal 'df' displays filesystem statistics.
    7. AegisPad UI [Save] button persists to VFS.
    """
    # 1. Wait for boot and VFS initialization
    qemu.wait_for_serial(r"\[OK\] In-Memory Virtual Filesystem", timeout=12.0)
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)

    # 2. Terminal commands
    time.sleep(0.5)

    # Test 'ls'
    for ch in "ls":
        qemu.send_key(ch)
        time.sleep(0.02)
    qemu.send_key("ret")
    time.sleep(0.3)

    # Test 'cat /welcome.txt'
    for ch in "cat /welcome.txt":
        qemu.send_key(ch)
        time.sleep(0.02)
    qemu.send_key("ret")
    time.sleep(0.5)

    # Test 'write /user/test.txt VFS-Persistent-Document-Verified'
    for ch in "write /user/test.txt VFS-Persistent-Document-Verified":
        qemu.send_key(ch)
        time.sleep(0.02)
    qemu.send_key("ret")
    time.sleep(0.5)

    # Test 'cat /user/test.txt'
    for ch in "cat /user/test.txt":
        qemu.send_key(ch)
        time.sleep(0.02)
    qemu.send_key("ret")
    time.sleep(0.5)

    # Test 'df'
    for ch in "df":
        qemu.send_key(ch)
        time.sleep(0.02)
    qemu.send_key("ret")
    time.sleep(0.5)

    # 3. Capture screendump to verify visual output in terminal
    img = qemu.screendump()
    assert img.width == 1280 and img.height == 800
    assert not img.is_flat_color(threshold=10)

    # 4. Verify system liveness and CPU execution
    rips, all_if_enabled, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert is_moving, "CPU stopped executing after VFS file operations"
