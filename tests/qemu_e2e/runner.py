"""Unified CLI Test Runner for AegisOS (Epsilon OS) QEMU E2E Test Suite.

Discovers and executes all bare-metal E2E test suites with rich diagnostics,
individual test timing, and ANSI colorized output.
"""

import argparse
import inspect
import os
import sys
import time
import traceback
from typing import Callable, List, Tuple

try:
    from .harness import QemuHarness
    from . import test_boot
    from . import test_framebuffer
    from . import test_fault_isolation
    from . import test_terminal
    from . import test_stability
    from . import test_selftest
    from . import test_frame_pacing
    from . import test_vfs
    from . import test_paint
    from . import test_file_manager
    from . import test_audio
    from . import test_window_snapping
    from . import test_settings
    from . import test_calculator
    from . import test_terminal_advanced
    from . import test_spotlight_and_browser
    from . import test_minesweeper
    from . import test_editor_advanced
    from . import test_synth
    from . import test_chat
    from . import test_elf_syscall
except ImportError:
    from tests.qemu_e2e.harness import QemuHarness
    from tests.qemu_e2e import test_boot
    from tests.qemu_e2e import test_framebuffer
    from tests.qemu_e2e import test_fault_isolation
    from tests.qemu_e2e import test_terminal
    from tests.qemu_e2e import test_stability
    from tests.qemu_e2e import test_selftest
    from tests.qemu_e2e import test_frame_pacing
    from tests.qemu_e2e import test_vfs
    from tests.qemu_e2e import test_paint
    from tests.qemu_e2e import test_file_manager
    from tests.qemu_e2e import test_audio
    from tests.qemu_e2e import test_window_snapping
    from tests.qemu_e2e import test_settings
    from tests.qemu_e2e import test_calculator
    from tests.qemu_e2e import test_terminal_advanced
    from tests.qemu_e2e import test_spotlight_and_browser
    from tests.qemu_e2e import test_minesweeper
    from tests.qemu_e2e import test_editor_advanced
    from tests.qemu_e2e import test_synth
    from tests.qemu_e2e import test_chat
    from tests.qemu_e2e import test_elf_syscall


# ANSI Color codes
GREEN = "\033[92m"
RED = "\033[91m"
YELLOW = "\033[93m"
CYAN = "\033[96m"
BOLD = "\033[1m"
RESET = "\033[0m"


TEST_REGISTRY: List[Tuple[str, Callable]] = [
    ("selftest::test_in_kernel_selftests", test_selftest.test_in_kernel_selftests),
    ("boot::test_boot_sequence", test_boot.test_boot_sequence),
    ("framebuffer::test_framebuffer_rendering", test_framebuffer.test_framebuffer_rendering),
    ("fault::test_boot_fault_isolation", test_fault_isolation.test_boot_fault_isolation),
    ("fault::test_crashtest_all_buttons", test_fault_isolation.test_crashtest_all_buttons),
    ("terminal::test_terminal_shell_interaction", test_terminal.test_terminal_shell_interaction),
    ("frame_pacing::test_frame_pacing_and_glyphs", test_frame_pacing.test_frame_pacing_and_glyphs),
    ("vfs::test_vfs_file_lifecycle", test_vfs.test_vfs_file_lifecycle),
    ("paint::test_paint_drawing_lifecycle", test_paint.test_paint_drawing_lifecycle),
    ("file_manager::test_file_manager_lifecycle", test_file_manager.test_file_manager_lifecycle),
    ("audio::test_pc_speaker_audio", test_audio.test_pc_speaker_audio),
    ("window_snapping::test_window_snapping_and_tiling", test_window_snapping.test_window_snapping_and_tiling),
    ("settings::test_system_settings_and_wallpaper", test_settings.test_system_settings_and_wallpaper),
    ("calculator::test_scientific_calculator_lifecycle", test_calculator.test_scientific_calculator_lifecycle),
    ("terminal_advanced::test_terminal_history_and_completion", test_terminal_advanced.test_terminal_history_and_completion),
    ("spotlight_and_browser::test_spotlight_and_browser_lifecycle", test_spotlight_and_browser.test_spotlight_and_browser_lifecycle),
    ("minesweeper::test_minesweeper_lifecycle", test_minesweeper.test_minesweeper_lifecycle),
    ("editor_advanced::test_editor_advanced_lifecycle", test_editor_advanced.test_editor_advanced_lifecycle),
    ("synth::test_synth_lifecycle", test_synth.test_synth_lifecycle),
    ("chat::test_chat_lifecycle", test_chat.test_chat_lifecycle),
    ("elf_syscall::test_elf_load_syscall_and_isolation", test_elf_syscall.test_elf_load_syscall_and_isolation),
    ("stability::test_clock_progress", test_stability.test_clock_progress),
    ("stability::test_input_flood_resilience", test_stability.test_input_flood_resilience),
]


def run_test(
    name: str,
    test_func: Callable,
    iso_path: str,
    accel: str,
    timeout: float,
    verbose: bool = False,
) -> Tuple[bool, float, str]:
    """Runs a single test case."""
    start_time = time.time()
    err_msg = ""
    success = False

    try:
        sig = inspect.signature(test_func)
        if len(sig.parameters) == 0:
            test_func()
            success = True
        else:
            with QemuHarness(iso_path=iso_path, accel=accel, timeout=timeout) as qemu:
                test_func(qemu)
                success = True
    except Exception as e:
        success = False
        tb = traceback.format_exc()
        err_msg = f"{e}\n\nTraceback:\n{tb}"

    elapsed = time.time() - start_time
    return success, elapsed, err_msg


def main():
    parser = argparse.ArgumentParser(
        description="AegisOS (Epsilon OS) Bare-Metal QEMU E2E Test Runner"
    )
    parser.add_argument(
        "--filter",
        "-k",
        type=str,
        default="",
        help="Filter test names matching substring",
    )
    parser.add_argument(
        "--iso",
        type=str,
        default="",
        help="Path to aegis_os.iso (default: repository root)",
    )
    parser.add_argument(
        "--accel",
        type=str,
        default="kvm",
        choices=["kvm", "tcg"],
        help="QEMU acceleration mode (default: kvm)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=25.0,
        help="Per-test timeout in seconds (default: 25.0)",
    )
    parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="Verbose logging output",
    )

    args = parser.parse_args()

    # Determine ISO path
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    iso_path = args.iso or os.path.join(repo_root, "aegis_os.iso")

    if not os.path.exists(iso_path):
        print(f"{RED}[FATAL] ISO image not found at: {iso_path}{RESET}")
        print("Run ./build_iso.sh first or specify --iso <path>.")
        sys.exit(1)

    # Filter tests
    selected_tests = [
        (name, fn)
        for (name, fn) in TEST_REGISTRY
        if not args.filter or args.filter.lower() in name.lower()
    ]

    if not selected_tests:
        print(f"{YELLOW}No tests matched filter: {args.filter!r}{RESET}")
        sys.exit(0)

    print(f"{BOLD}================================================================={RESET}")
    print(f"{BOLD}       AegisOS (Epsilon OS) Bare-Metal QEMU E2E Test Suite       {RESET}")
    print(f"{BOLD}================================================================={RESET}")
    print(f"ISO Target:    {iso_path}")
    print(f"Acceleration:  {args.accel}")
    print(f"Test Count:    {len(selected_tests)}")
    print(f"-----------------------------------------------------------------")

    passed = 0
    failed = 0
    total_start = time.time()
    failures = []

    for idx, (name, test_fn) in enumerate(selected_tests, 1):
        print(f"[{idx}/{len(selected_tests)}] Running {CYAN}{name}{RESET} ...", end="", flush=True)
        ok, elapsed, err = run_test(
            name,
            test_fn,
            iso_path=iso_path,
            accel=args.accel,
            timeout=args.timeout,
            verbose=args.verbose,
        )

        if ok:
            passed += 1
            print(f"\r[{idx}/{len(selected_tests)}] {GREEN}[PASS]{RESET} {name} ({elapsed:.2f}s)")
        else:
            failed += 1
            print(f"\r[{idx}/{len(selected_tests)}] {RED}[FAIL]{RESET} {name} ({elapsed:.2f}s)")
            failures.append((name, err))

    total_elapsed = time.time() - total_start

    print(f"-----------------------------------------------------------------")
    print(f"{BOLD}SUMMARY:{RESET}")
    print(
        f"Total: {len(selected_tests)} | "
        f"{GREEN}Passed: {passed}{RESET} | "
        f"{RED if failed else GREEN}Failed: {failed}{RESET} | "
        f"Elapsed: {total_elapsed:.2f}s"
    )

    if failures:
        print(f"\n{BOLD}{RED}=== TEST FAILURES ({len(failures)}) ==={RESET}")
        for name, err in failures:
            print(f"\n{BOLD}{RED}FAILED: {name}{RESET}")
            print(f"{err}")
        print(f"{BOLD}================================================================={RESET}")
        sys.exit(1)
    else:
        print(f"\n{BOLD}{GREEN}ALL {passed} TESTS PASSED SUCCESSFULLY!{RESET}")
        print(f"{BOLD}================================================================={RESET}")
        sys.exit(0)


if __name__ == "__main__":
    main()
