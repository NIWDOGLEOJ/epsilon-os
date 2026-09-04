#!/usr/bin/env bash
# =============================================================================
# AegisOS Bare-Metal In-Kernel Self-Test Runner
# Builds kernel with --features selftest and executes in QEMU with isa-debug-exit.
# =============================================================================

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ISO_PATH="$SCRIPT_DIR/aegis_os_selftest.iso"

echo "=== Building AegisOS ISO with In-Kernel Self-Tests ==="
ISO_OUTPUT="$ISO_PATH" CARGO_FEATURES="selftest" bash "$SCRIPT_DIR/build_iso.sh"

echo ""
echo "=== Running In-Kernel Bare-Metal Self-Tests in QEMU ==="
echo "    Device: isa-debug-exit (port 0xf4) | Display: none | Serial: stdout"
echo ""

set +e
qemu-system-x86_64 \
    -cdrom "$ISO_PATH" \
    -m 4G \
    -accel kvm \
    -accel tcg \
    -vga std \
    -display none \
    -serial stdio \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -no-reboot

QEMU_EXIT_CODE=$?
set -e

echo ""
echo "QEMU Process Exit Code: $QEMU_EXIT_CODE"

# QEMU isa-debug-exit maps value `v` to `(v << 1) | 1`
# Success: (0x10 << 1) | 1 = 33 (0x21)
# Failure: (0x11 << 1) | 1 = 35 (0x23)
if [ "$QEMU_EXIT_CODE" -eq 33 ]; then
    echo "======================================================="
    echo " [PASS] In-Kernel Self-Tests Completed Successfully!  "
    echo "======================================================="
    exit 0
elif [ "$QEMU_EXIT_CODE" -eq 35 ]; then
    echo "======================================================="
    echo " [FAIL] In-Kernel Self-Tests FAILED! (Status 35)      "
    echo "======================================================="
    exit 1
else
    echo "======================================================="
    echo " [WARN] Unexpected exit code: $QEMU_EXIT_CODE         "
    echo "======================================================="
    exit "$QEMU_EXIT_CODE"
fi
