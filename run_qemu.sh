#!/bin/bash
# =============================================================================
# AegisOS QEMU Runner Script
# Builds the kernel, packages the ISO, and launches in QEMU with serial logging.
# =============================================================================

set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
ISO_PATH="$PROJECT_DIR/aegis_os.iso"

# Build the ISO first
echo "=== Building AegisOS ISO... ==="
bash "$PROJECT_DIR/build_iso.sh"

if [ ! -f "$ISO_PATH" ]; then
    echo "[FATAL] ISO not found at $ISO_PATH"
    exit 1
fi

# Launch QEMU
echo ""
echo "=== Launching AegisOS in QEMU ==="
echo "    RAM: 4G | Display: VGA Std | Acceleration: KVM/TCG | Serial: stdio"
echo "    Press Ctrl+A then X to exit QEMU (or close the window)"
echo ""

export DISPLAY="${DISPLAY:-:0}"
export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-0}"

qemu-system-x86_64 \
    -cdrom "$ISO_PATH" \
    -m 4G \
    -cpu host \
    -vga std \
    -accel kvm \
    -accel tcg \
    -serial stdio \
    -no-reboot \
    -no-shutdown
