"""Core QEMU E2E Test Harness for AegisOS (Epsilon OS).

Manages QEMU process lifecycle, UNIX domain monitor socket interaction,
serial log parsing, binary PPM framebuffer capture and inspection,
and PS/2 mouse/keyboard input injection.
"""

import os
import re
import socket
import subprocess
import time
import uuid
from typing import Dict, List, Optional, Set, Tuple


class PpmImage:
    """Parser and inspector for binary PPM (P6) framebuffer dumps."""

    def __init__(self, width: int, height: int, maxval: int, data: bytes):
        self.width = width
        self.height = height
        self.maxval = maxval
        self.data = data

    @classmethod
    def from_file(cls, path: str) -> "PpmImage":
        with open(path, "rb") as f:
            header = f.readline().strip()
            if header != b"P6":
                raise ValueError(f"Unsupported PPM header: {header!r}, expected b'P6'")

            line = f.readline()
            while line.startswith(b"#"):
                line = f.readline()

            dims = line.strip().split()
            width, height = int(dims[0]), int(dims[1])

            maxval_line = f.readline()
            while maxval_line.startswith(b"#"):
                maxval_line = f.readline()
            maxval = int(maxval_line.strip())

            data = f.read()
            expected_bytes = width * height * 3
            if len(data) < expected_bytes:
                raise ValueError(
                    f"Truncated PPM: expected {expected_bytes} bytes, got {len(data)}"
                )

            return cls(width, height, maxval, data)

    def get_pixel(self, x: int, y: int) -> Tuple[int, int, int]:
        """Returns (R, G, B) tuple for pixel at (x, y)."""
        if not (0 <= x < self.width and 0 <= y < self.height):
            raise IndexError(f"Coordinates ({x}, {y}) out of bounds ({self.width}x{self.height})")
        offset = (y * self.width + x) * 3
        return (self.data[offset], self.data[offset + 1], self.data[offset + 2])

    def unique_colors(self, step: int = 4) -> Set[Tuple[int, int, int]]:
        """Samples the image with given stride and returns set of unique RGB colors."""
        colors = set()
        for y in range(0, self.height, step):
            for x in range(0, self.width, step):
                colors.add(self.get_pixel(x, y))
        return colors

    def is_flat_color(self, threshold: int = 5) -> bool:
        """Returns True if the image is essentially a single flat solid color."""
        sample_colors = self.unique_colors(step=16)
        return len(sample_colors) <= threshold

    def color_variance(self, step: int = 8) -> float:
        """Computes luminance variance across the image."""
        lums = []
        for y in range(0, self.height, step):
            for x in range(0, self.width, step):
                r, g, b = self.get_pixel(x, y)
                lums.append(0.299 * r + 0.587 * g + 0.114 * b)
        if not lums:
            return 0.0
        mean = sum(lums) / len(lums)
        variance = sum((l - mean) ** 2 for l in lums) / len(lums)
        return variance


class QemuHarness:
    """Manages headless QEMU instance with monitor socket and serial console capture."""

    def __init__(
        self,
        iso_path: Optional[str] = None,
        memory: str = "4G",
        vga: str = "std",
        accel: str = "kvm",
        timeout: float = 30.0,
    ):
        base_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
        self.iso_path = iso_path or os.path.join(base_dir, "aegis_os.iso")
        self.memory = memory
        self.vga = vga
        self.accel = accel
        self.default_timeout = timeout

        session_id = uuid.uuid4().hex[:8]
        self.sock_path = f"/tmp/aegis_e2e_{session_id}.sock"
        self.serial_path = f"/tmp/aegis_e2e_{session_id}.log"
        self.ppm_path = f"/tmp/aegis_e2e_{session_id}.ppm"

        self.proc: Optional[subprocess.Popen] = None
        self.mon_sock: Optional[socket.socket] = None

    def __enter__(self) -> "QemuHarness":
        self.start()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.stop()

    def start(self):
        """Launches QEMU in headless mode and connects to monitor socket."""
        if not os.path.isfile(self.iso_path):
            raise FileNotFoundError(f"ISO image not found at {self.iso_path}")

        # Remove stale temp files
        for p in [self.sock_path, self.serial_path, self.ppm_path]:
            if os.path.exists(p):
                try:
                    os.remove(p)
                except OSError:
                    pass

        cmd = [
            "qemu-system-x86_64",
            "-cdrom", self.iso_path,
            "-m", self.memory,
            "-accel", self.accel,
            "-accel", "tcg",
            "-vga", self.vga,
            "-display", "none",
            "-serial", f"file:{self.serial_path}",
            "-monitor", f"unix:{self.sock_path},server,nowait",
            "-no-reboot",
            "-no-shutdown",
        ]

        self.proc = subprocess.Popen(
            cmd,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )

        # Wait for monitor UNIX domain socket to appear
        start_t = time.time()
        connected = False
        while time.time() - start_t < 10.0:
            if os.path.exists(self.sock_path):
                try:
                    self.mon_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    self.mon_sock.connect(self.sock_path)
                    self.mon_sock.settimeout(3.0)
                    # Consume initial QEMU monitor greeting
                    try:
                        self.mon_sock.recv(1024)
                    except socket.timeout:
                        pass
                    connected = True
                    break
                except (socket.error, ConnectionRefusedError):
                    time.sleep(0.05)
            time.sleep(0.05)

        if not connected:
            self.stop()
            raise TimeoutError(f"Timed out connecting to QEMU monitor socket at {self.sock_path}")

    def stop(self):
        """Cleanly stops QEMU and removes temporary files."""
        if self.mon_sock:
            try:
                self.mon_sock.sendall(b"quit\n")
                time.sleep(0.1)
                self.mon_sock.close()
            except Exception:
                pass
            self.mon_sock = None

        if self.proc:
            try:
                self.proc.wait(timeout=2.0)
            except subprocess.TimeoutExpired:
                self.proc.terminate()
                try:
                    self.proc.wait(timeout=2.0)
                except subprocess.TimeoutExpired:
                    self.proc.kill()
                    self.proc.wait()
            self.proc = None

        for p in [self.sock_path, self.ppm_path]:
            if os.path.exists(p):
                try:
                    os.remove(p)
                except OSError:
                    pass

    def execute_monitor(self, cmd: str, timeout: float = 2.0) -> str:
        """Sends command to QEMU monitor and collects output."""
        if not self.mon_sock:
            raise RuntimeError("Monitor socket is not connected")

        self.mon_sock.settimeout(timeout)
        self.mon_sock.sendall((cmd.strip() + "\n").encode("utf-8"))

        # In QEMU monitor, prompts end with '(qemu) '
        output = b""
        start_t = time.time()
        while time.time() - start_t < timeout:
            try:
                chunk = self.mon_sock.recv(4096)
                if not chunk:
                    break
                output += chunk
                if b"(qemu) " in output:
                    break
            except socket.timeout:
                break

        return output.decode("utf-8", errors="replace")

    def get_serial_log(self) -> str:
        """Reads complete serial console log written so far."""
        if not os.path.exists(self.serial_path):
            return ""
        try:
            with open(self.serial_path, "r", encoding="utf-8", errors="replace") as f:
                return f.read()
        except OSError:
            return ""

    def get_serial_lines(self) -> List[str]:
        """Returns list of lines logged to serial."""
        return [l.strip() for l in self.get_serial_log().splitlines() if l.strip()]

    def wait_for_serial(self, pattern: str, timeout: Optional[float] = None) -> str:
        """Waits until pattern appears in serial log or times out."""
        limit = timeout if timeout is not None else self.default_timeout
        start_t = time.time()
        compiled = re.compile(pattern)

        while time.time() - start_t < limit:
            content = self.get_serial_log()
            for line in content.splitlines():
                if compiled.search(line):
                    return line
            time.sleep(0.1)

        log_tail = "\n".join(self.get_serial_lines()[-20:])
        raise TimeoutError(
            f"Pattern {pattern!r} not found in serial log within {limit}s.\nRecent log:\n{log_tail}"
        )

    def read_registers(self) -> Dict[str, any]:
        """Queries QEMU 'info registers' and returns parsed register values."""
        raw = self.execute_monitor("info registers")
        result = {"raw": raw, "rip": None, "rflags": None, "if_flag": False}

        for line in raw.splitlines():
            if "RIP=" in line and "RFL=" in line:
                for token in line.split():
                    if token.startswith("RIP="):
                        result["rip"] = int(token.split("=")[1], 16)
                    elif token.startswith("RFL="):
                        rfl = int(token.split("=")[1], 16)
                        result["rflags"] = rfl
                        # Bit 9 (0x200) is the Interrupt Flag (IF)
                        result["if_flag"] = bool(rfl & 0x200)
            if "CS=" in line:
                for token in line.split():
                    if token.startswith("CS="):
                        result["cs"] = int(token.split("=")[1], 16)
            if "CPL=" in line:
                for token in line.split():
                    if token.startswith("CPL="):
                        result["cpl"] = int(token.split("=")[1])

        return result

    def sample_rip_stability(
        self, samples: int = 5, interval: float = 0.1
    ) -> Tuple[List[int], bool, bool]:
        """Samples RIP and RFLAGS.IF across multiple intervals.

        Returns (rips, interrupts_active, is_moving).
        """
        rips = []
        if_count = 0

        for _ in range(samples):
            regs = self.read_registers()
            if regs["rip"] is not None:
                rips.append(regs["rip"])
            if regs.get("if_flag", False):
                if_count += 1
            time.sleep(interval)

        distinct = len(set(rips))
        is_moving = distinct > 1
        # True deadlock is when interrupts remain permanently disabled across all samples
        interrupts_active = if_count > 0
        return rips, interrupts_active, is_moving

    def mouse_move(self, dx: int, dy: int):
        """Sends relative mouse movement."""
        self.execute_monitor(f"mouse_move {dx} {dy}")

    def mouse_click(self, button: int = 1, hold_sec: float = 0.05):
        """Clicks specified mouse button (1=Left, 2=Middle, 4=Right)."""
        self.execute_monitor(f"mouse_button {button}")
        time.sleep(hold_sec)
        self.execute_monitor("mouse_button 0")

    def send_key(self, key: str):
        """Sends a single keypress via QEMU sendkey."""
        self.execute_monitor(f"sendkey {key}")
        time.sleep(0.02)

    def send_string(self, text: str, delay_between_keys: float = 0.03):
        """Types a string into the active window by mapping chars to QEMU key codes."""
        key_map = {
            " ": "spc",
            "\n": "ret",
            "\r": "ret",
            "\t": "tab",
            "-": "minus",
            "=": "equal",
            "+": "shift-equal",
            "_": "shift-minus",
            "*": "shift-8",
            "/": "slash",
            ":": "shift-semicolon",
            ";": "semicolon",
            "~": "shift-grave_accent",
            ".": "dot",
            ",": "comma",
        }

        for ch in text:
            if ch in key_map:
                self.send_key(key_map[ch])
            elif ch.isupper():
                self.send_key(f"shift-{ch.lower()}")
            elif ch.isalnum():
                self.send_key(ch)
            else:
                self.send_key(ch)
            time.sleep(delay_between_keys)

    def screendump(self, output_path: Optional[str] = None) -> PpmImage:
        """Captures framebuffer screenshot via monitor and returns parsed PpmImage."""
        target = output_path or self.ppm_path
        if os.path.exists(target):
            try:
                os.remove(target)
            except OSError:
                pass

        self.execute_monitor(f"screendump {target}")

        # Wait for file to be written and flushed
        start_t = time.time()
        while time.time() - start_t < 3.0:
            if os.path.exists(target) and os.path.getsize(target) > 1024:
                break
            time.sleep(0.05)

        if not os.path.exists(target):
            raise RuntimeError(f"screendump failed to produce file at {target}")

        return PpmImage.from_file(target)
