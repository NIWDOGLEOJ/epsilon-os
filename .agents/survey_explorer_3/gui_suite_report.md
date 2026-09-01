# AegisOS GUI, Application Suite & Build Pipeline Specification Report

**Author**: survey_explorer_3 (GUI & System Suite Spec Miner)  
**Date**: 2026-08-30  
**Target Architecture**: x86_64 (`no_std`)  
**Bootloader Protocol**: Limine Boot Protocol v2  
**Scope**: Framebuffer Graphics Engine, Desktop Environment (R4), 5 Core Applications & Demo Suite (R5), and Build & ISO Packaging Toolchain (R6)

---

## 1. Executive Summary & Architecture Overview

AegisOS couples a crash-resilient x86_64 microkernel with a macOS-inspired graphical desktop environment and an integrated system application suite. The system guarantees that userspace application crashes (such as page faults, divide-by-zero, invalid opcodes, or out-of-bounds writes) are isolated to the offending process, leaving the graphical desktop, top menu bar, Activity Monitor, and mouse cursor fully operational.

The graphical subsystem and application suite are designed with strict performance, stability, and memory footprint constraints:
- **Zero Kernel Panics on App Faults**: Faulting application windows cleanly close or display crash telemetry; desktop compositor continues unaffected at 60 FPS.
- **Memory Footprint**: Total system RAM consumption at idle desktop is **< 60 MB** (measured and displayed live in the Activity Monitor and Top Menu Bar) on 512MB–4GB hardware.
- **Rendering Architecture**: High-speed double-buffered linear 32-bit ARGB/XRGB framebuffer with dirty rectangle tracking and fast 64-bit scanline copying.
- **Input Pipeline**: Interrupt-driven PS/2 mouse packet state machine (3-byte decoding with sign extension) and PS/2 Set 1/2 keyboard scancode translation with focus-routed event dispatch.

```
+-------------------------------------------------------------------------------+
|                             AegisOS GUI Architecture                          |
+-------------------------------------------------------------------------------+
|  +-------------------------------------------------------------------------+  |
|  |       Top System Menu Bar (24px): Logo | Active App | Uptime | CPU/RAM  |  |
|  +-------------------------------------------------------------------------+  |
|                                                                               |
|  +---------------------------+   +-----------------------------------------+  |
|  | Crash-Test Demo App       |   | Activity Monitor                        |  |
|  | [Null Ptr] [Div0] [UD2]   |   | [CPU Gauge] [RAM Footprint <60MB Graph] |  |
|  | [OOB Write]               |   | [Process Table: PID, State, Kill]       |  |
|  +---------------------------+   +-----------------------------------------+  |
|                                                                               |
|  +---------------------------+   +-----------------------------------------+  |
|  | Interactive Terminal Shell|   | Text Editor (AegisPad)                  |  |
|  | ps / kill / free / echo   |   | Multiline text buffer, gutter & cursor  |  |
|  | run / clear / reboot      |   +-----------------------------------------+  |
|  +---------------------------+   +-----------------------------------------+  |
|                                  | About AegisOS Modal Dialog              |  |
|                                  +-----------------------------------------+  |
|                                                                               |
|  +-------------------------------------------------------------------------+  |
|  |            Launcher Dock (Bottom Center): 5 Clickable App Icons         |  |
|  +-------------------------------------------------------------------------+  |
|                                                                               |
|  +-------------------------------------------------------------------------+  |
|  |  Window Manager & Compositor (Z-Order, Dragging, Traffic Lights, Events)|  |
|  +-------------------------------------------------------------------------+  |
|  |  2D Graphics Engine (Double Buffering, Blitter, Primitives, 8x16 Font)  |  |
|  +-------------------------------------------------------------------------+  |
|  |  Limine Framebuffer Driver (Linear 32-bit ARGB/XRGB @ 1024x768x32)      |  |
|  +-------------------------------------------------------------------------+  |
+-------------------------------------------------------------------------------+
```

---

## 2. Double-Buffered Linear RGB Framebuffer Engine

### 2.1 Limine Framebuffer Interface
AegisOS requests a high-resolution linear framebuffer from the Limine bootloader using the Limine Framebuffer Request feature.

#### 2.1.1 Request & Response Layout
```rust
use limine::request::FramebufferRequest;
use limine::response::FramebufferResponse;

#[used]
#[link_section = ".limine_requests"]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();
```

When Limine boots the kernel, it provides a `FramebufferResponse` containing an array of pointers to `limine::framebuffer::Framebuffer` structs:
- `address: *mut u8`: Virtual address of the linear framebuffer in the Higher-Half Direct Map (HHDM).
- `width: u64`: Horizontal resolution in pixels (target default: `1024`).
- `height: u64`: Vertical resolution in pixels (target default: `768`).
- `pitch: u64`: Bytes per scanline (typically `width * 4`, e.g., `4096` bytes).
- `bpp: u16`: Bits per pixel (standard: `32`).
- `memory_model: u8`: RGB model (`1`).
- `red_mask_size: u8`, `red_mask_shift: u8`: Standard 8 bits at shift 16 (`0x00FF0000`).
- `green_mask_size: u8`, `green_mask_shift: u8`: Standard 8 bits at shift 8 (`0x0000FF00`).
- `blue_mask_size: u8`, `blue_mask_shift: u8`: Standard 8 bits at shift 0 (`0x000000FF`).

### 2.2 Double-Buffering & Backbuffer Memory Management
To eliminate screen tearing, flickering, and partial redraw artifacts, direct writes to the hardware framebuffer are prohibited. All drawing operations target an offscreen backbuffer in system RAM.

#### 2.2.1 Memory Footprint Calculation
- Standard Resolution: $1024 \times 768 \times 32\text{ bpp}$
- Backbuffer Size: $1024 \times 768 \times 4\text{ bytes} = 3,145,728\text{ bytes} \approx 3.0\text{ MB}$.
- Frontbuffer Size: Hardware VRAM mapped in HHDM $\approx 3.0\text{ MB}$.
- **Total Framebuffer RAM Usage**: $\approx 3.0\text{ MB}$ (allocated on kernel heap or static BSS frame pool).
- Even with dynamic UI buffers, total GUI footprint is $< 8\text{ MB}$, well below the $\mathbf{< 60\text{ MB}}$ idle budget.

#### 2.2.2 Fast Scanline Blitter (SIMD / 64-bit Copy)
```rust
pub struct Framebuffer {
    pub frontbuffer: *mut u32,
    pub backbuffer: &'static mut [u32],
    pub width: usize,
    pub height: usize,
    pub pitch_pixels: usize,
    pub dirty_rect: Option<Rect>,
}

impl Framebuffer {
    /// Present backbuffer to frontbuffer
    pub fn swap_buffers(&mut self) {
        if let Some(rect) = self.dirty_rect.take() {
            let start_x = rect.x.clamp(0, self.width as i32) as usize;
            let end_x = (rect.x + rect.width as i32).clamp(0, self.width as i32) as usize;
            let start_y = rect.y.clamp(0, self.height as i32) as usize;
            let end_y = (rect.y + rect.height as i32).clamp(0, self.height as i32) as usize;
            let copy_width = end_x.saturating_sub(start_x);

            if copy_width == 0 || start_y >= end_y {
                return;
            }

            for y in start_y..end_y {
                let src_offset = y * self.width + start_x;
                let dst_ptr = unsafe {
                    self.frontbuffer.add(y * self.pitch_pixels + start_x)
                };
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        self.backbuffer[src_offset..].as_ptr(),
                        dst_ptr,
                        copy_width,
                    );
                }
            }
        }
    }
}
```

### 2.3 2D Vector Primitives & Alpha Blending
The graphics engine provides standard 2D primitives:

#### 2.3.1 Color Representation & Alpha Blending
```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self { Self { r, g, b, a: 255 } }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self { Self { r, g, b, a } }
    
    #[inline(always)]
    pub fn to_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    #[inline(always)]
    pub fn blend(src: Color, dst: Color) -> Color {
        if src.a == 255 { return src; }
        if src.a == 0 { return dst; }
        let alpha = src.a as u32;
        let inv_alpha = 255 - alpha;
        Color {
            r: (((src.r as u32 * alpha) + (dst.r as u32 * inv_alpha)) / 255) as u8,
            g: (((src.g as u32 * alpha) + (dst.g as u32 * inv_alpha)) / 255) as u8,
            b: (((src.b as u32 * alpha) + (dst.b as u32 * inv_alpha)) / 255) as u8,
            a: 255,
        }
    }
}
```

#### 2.3.2 Primitives Catalog
- `draw_pixel(x, y, color)`: Bounds-checked pixel write with alpha blending.
- `draw_rect(rect, color)`: Filled rectangle.
- `draw_rect_outline(rect, border_color, thickness)`.
- `draw_rounded_rect(rect, radius, color)`: Filled rectangle with anti-aliased or quadrant-rounded corners (critical for macOS windows, dock, and buttons).
- `draw_circle(cx, cy, radius, color)`: Filled circle (used for traffic-light buttons and status badges).
- `draw_gradient_v(rect, top_color, bottom_color)`: Linear vertical gradient for titlebars and menu bars.
- `draw_shadow(rect, radius, opacity)`: Translucent blurred drop shadow around windows for depth.

### 2.4 Embedded Bitmap Font Rendering
AegisOS embeds an 8x16 VGA / PC Screen Font (PSF) directly in the kernel binary as static data (`&[u8; 4096]`).

#### 2.4.1 Font Data Layout & Character Drawing
Each ASCII glyph occupies 16 consecutive bytes (1 byte per row, 8 bits wide, MSB is leftmost pixel).

```rust
pub const FONT_WIDTH: usize = 8;
pub const FONT_HEIGHT: usize = 16;

pub fn draw_char(fb: &mut Framebuffer, x: i32, y: i32, c: u8, fg: Color, bg: Option<Color>) {
    let glyph_index = if c < 128 { c as usize } else { b'?' as usize };
    let glyph = &FONT_8X16[glyph_index * FONT_HEIGHT..(glyph_index + 1) * FONT_HEIGHT];

    for (row, &byte) in glyph.iter().enumerate() {
        let py = y + row as i32;
        for col in 0..8 {
            let px = x + col as i32;
            let bit = (byte >> (7 - col)) & 1;
            if bit != 0 {
                fb.draw_pixel(px, py, fg);
            } else if let Some(bg_color) = bg {
                fb.draw_pixel(px, py, bg_color);
            }
        }
    }
}

pub fn draw_string(fb: &mut Framebuffer, mut x: i32, y: i32, text: &str, fg: Color, bg: Option<Color>) {
    for byte in text.bytes() {
        if byte == b'\n' {
            break;
        }
        draw_char(fb, x, y, byte, fg, bg);
        x += FONT_WIDTH as i32;
    }
}
```

---

## 3. Desktop Environment Specification (R4)

### 3.1 Visual Theme & Color System (macOS Cupertino Dark)

| Component | Color Hex | RGB Value | Purpose / Description |
|---|---|---|---|
| **Desktop Background** | `#1E222A` to `#14161C` | `(30, 34, 42)` | Deep slate gradient with subtle radial glow |
| **Top Menu Bar** | `#18181A` (92% Alpha) | `(24, 24, 26, 235)` | 24px top bar with bottom 1px divider `#2E323A` |
| **Window Background** | `#21252B` | `(33, 37, 43)` | Main client area of application windows |
| **Active Titlebar** | `#2C313C` to `#242830` | `(44, 49, 60)` | Gradient titlebar for focused window |
| **Inactive Titlebar** | `#1E2227` | `(30, 34, 39)` | Dimmed titlebar for unfocused window |
| **Window Border** | `#3B4048` | `(59, 64, 72)` | 1px subtle stroke with 4px drop shadow |
| **Close Button (Red)** | `#FF5F56` | `(255, 95, 86)` | Traffic-light close (hover `#FF3B30`) |
| **Minimize Button (Yellow)**| `#FFBD2E` | `(255, 189, 46)` | Traffic-light minimize (hover `#FF9500`) |
| **Maximize Button (Green)**| `#27C93F` | `(39, 201, 63)` | Traffic-light zoom/maximize (hover `#28CD41`) |
| **Launcher Dock** | `#1A1D24` (88% Alpha) | `(26, 29, 36, 225)` | Rounded translucent pill container at screen bottom |
| **Dock Border** | `#3E4451` | `(62, 68, 81)` | 1px rounded stroke with 12px corner radius |
| **Accent / Button Blue** | `#007AFF` | `(0, 122, 255)` | macOS primary action blue |
| **Text Primary** | `#E5E5E5` | `(229, 229, 229)` | High-contrast white/light grey text |
| **Text Secondary / Dim** | `#8A919E` | `(138, 145, 158)` | Labels, status text, subtitles |

---

### 3.2 Top System Menu Bar (24px)
The Top Menu Bar is anchored at `y = 0..24` across the entire screen width (`1024px`).

```
+----------------------------------------------------------------------------------------------------+
| [Shield] AegisOS   Terminal   File  Edit  View  Help         [CPU:  8%] [RAM: 38.4MB] [ 00:04:12 ] |
+----------------------------------------------------------------------------------------------------+
  (x=8)   (x=28)     (x=100)                                   (x=760)    (x=840)       (x=940)
```

#### Menu Bar Components:
1. **OS Logo**: Aegis Shield Icon rendered at `(x = 8, y = 4)`, size 16x16.
2. **System Title**: `"AegisOS"` in bold primary font at `(x = 28, y = 4)`.
3. **Active Application Indicator**: Displays the title of the currently focused window (e.g. `"Activity Monitor"`, `"Terminal"`, `"Crash-Test Demo"`) at `(x = 100, y = 4)` with bold white text.
4. **Application Menus**: Contextual items (`"File"`, `"Edit"`, `"View"`, `"Window"`, `"Help"`).
5. **Real-Time Telemetry Badges (Right Aligned)**:
   - **CPU Badge**: `[CPU:  8%]`. Background `#2E3440`, text `#50FA7B` (< 50%) / `#F1FA8C` (< 80%) / `#FF5555` (>= 80%).
   - **RAM Footprint Badge**: `[RAM: 38.4 MB]`. Directly proves that the OS satisfies the **< 60 MB RAM** constraint at idle.
   - **Uptime Clock**: `[ 00:12:45 ]` (HH:MM:SS) updated every second by system timer.

---

### 3.3 Floating Window Manager (WM)

#### 3.3.1 Window Structure & State
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppId {
    CrashTest,
    ActivityMonitor,
    Terminal,
    AegisPad,
    AboutDialog,
}

pub struct Window {
    pub id: u32,
    pub app_id: AppId,
    pub title: &'static str,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_focused: bool,
    pub is_dragging: bool,
    pub is_minimized: bool,
    pub is_closed: bool,
    pub drag_start_mouse_x: i32,
    pub drag_start_mouse_y: i32,
    pub drag_start_win_x: i32,
    pub drag_start_win_y: i32,
    pub z_order: usize,
    pub pid: Option<u32>,
}
```

#### 3.3.2 Draggable Titlebars & Window Bounds
- **Titlebar Height**: 24 pixels (`y = win.y .. win.y + 24`).
- **Drag Threshold & Clamping**:
  - When mouse is clicked inside titlebar (excluding traffic light buttons): `is_dragging = true`.
  - Coordinates are clamped so windows cannot be lost off-screen:
    $$\text{win.x} = \text{clamp}(\text{mouse\_x} - \text{drag\_offset\_x},\, -(\text{win.width} - 40),\, \text{screen\_width} - 40)$$
    $$\text{win.y} = \text{clamp}(\text{mouse\_y} - \text{drag\_offset\_y},\, 24,\, \text{screen\_height} - 30)$$

#### 3.3.3 Traffic-Light Close / Minimize / Zoom Buttons
- **Layout**: 3 circular buttons at the top-left of the titlebar:
  - Close (Red): Center at `(win.x + 16, win.y + 12)`, Radius 6px.
  - Minimize (Yellow): Center at `(win.x + 32, win.y + 12)`, Radius 6px.
  - Maximize (Green): Center at `(win.x + 48, win.y + 12)`, Radius 6px.
- **Actions**:
  - **Red Button Click**: Closes window, frees window state, sends `SIGKILL`/`SIGTERM` to associated userspace PID.
  - **Yellow Button Click**: Sets `is_minimized = true`, minimizes window into the bottom dock with animation or instant hide.
  - **Green Button Click**: Toggles between stored window size and maximized bounds `(0, 24, screen_width, screen_height - 24 - DOCK_HEIGHT)`.

#### 3.3.4 Z-Ordering & Focus Dispatch
- Windows are stored in a Z-order list (index `0` is lowest background window, index `N-1` is active foreground window).
- On MouseDown:
  1. Iterate windows in reverse Z-order (`N-1` down to `0`).
  2. If click falls within `(win.x, win.y, win.width, win.height)`:
     - Bring window to top: move to index `N-1`.
     - Set `win.is_focused = true`, set all other windows `is_focused = false`.
     - Route event to this window and stop search.

---

### 3.4 Launcher Dock at Bottom
- **Position & Dimensions**:
  - Width: `320px`, Height: `48px`, Corner Radius: `12px`.
  - Screen Placement: Centered horizontally at `x = (1024 - 320) / 2 = 352`, `y = 768 - 48 - 8 = 712`.
- **Dock Icons (5 Core Apps)**:
  1. **Crash-Test**: Hazard/Bug Icon (Red/Amber icon).
  2. **Activity Monitor**: Real-time Pulse / Activity Graph Icon (Green pulse line).
  3. **Terminal**: Dark console prompt `>_` (Black/Green terminal icon).
  4. **AegisPad**: Notepad & Pencil Icon (Blue note icon).
  5. **About AegisOS**: Golden Aegis Shield Icon.
- **Interaction & Running Indicators**:
  - Hovering over an icon displays an app name tooltip above the dock.
  - An active application renders a small 3px white/blue dot below its icon in the dock.
  - Clicking an icon:
    - If app is running and minimized: restores and focuses window.
    - If app is running and open: brings to foreground focus.
    - If app is not running: launches new process instance and opens window.

---

### 3.5 PS/2 Mouse & Keyboard Drivers

#### 3.5.1 PS/2 Mouse Packet Decoder (3-Byte Protocol)
- **Hardware Ports**: Data Port `0x60`, Status/Command Port `0x64`, IRQ 12.
- **Packet Format**:
  ```
  Byte 0: [ Y_ovf | X_ovf | Y_sign | X_sign | Always_1 | Mid_btn | Right_btn | Left_btn ]
  Byte 1: [ X Movement Delta (8-bit) ]
  Byte 2: [ Y Movement Delta (8-bit) ]
  ```
- **Sign Extension & Coordinate Transformation**:
  ```rust
  pub struct MousePacket {
      pub left_button: bool,
      pub right_button: bool,
      pub middle_button: bool,
      pub dx: i32,
      pub dy: i32,
  }

  pub fn parse_ps2_packet(bytes: [u8; 3]) -> Option<MousePacket> {
      // Bit 3 MUST be 1; if not, stream is out of sync
      if (bytes[0] & 0x08) == 0 {
          return None;
      }

      let left = (bytes[0] & 0x01) != 0;
      let right = (bytes[0] & 0x02) != 0;
      let middle = (bytes[0] & 0x04) != 0;

      let mut dx = bytes[1] as i32;
      if (bytes[0] & 0x10) != 0 {
          dx |= !0xFF; // Sign extend negative X
      }

      let mut dy = bytes[2] as i32;
      if (bytes[0] & 0x20) != 0 {
          dy |= !0xFF; // Sign extend negative Y
      }

      // PS/2 reports positive Y upwards; screen space is positive downwards
      Some(MousePacket {
          left_button: left,
          right_button: right,
          middle_button: middle,
          dx,
          dy: -dy,
      })
  }
  ```

#### 3.5.2 Mouse Cursor Rendering
- Standard macOS-style arrow cursor: 12x18 bitmap.
- Tip Hotspot: `(0, 0)`.
- Black 1px outline with pure white `#FFFFFF` fill and transparent background.
- Redrawn cleanly on top of the backbuffer before each frame swap.

#### 3.5.3 PS/2 Keyboard Scancode Decoder (Set 1)
- **Hardware Ports**: Data Port `0x60`, IRQ 1.
- **Decoding Engine**:
  - Make codes (`0x01` to `0x58`): Key pressed.
  - Break codes (`scancode & 0x80 != 0`): Key released (`make_code = scancode - 0x80`).
  - Extended prefix `0xE0`: Handles arrow keys (Up `0x48`, Down `0x50`, Left `0x4B`, Right `0x4D`), Delete `0x53`.
  - Shift / CapsLock State Machine: Translates alphabet `a-z` to `A-Z`, numbers to symbols (`1` -> `!`, `2` -> `@`, etc.).
- **Focus-Based Routing**:
  Keyboard events are dispatched directly to `WindowManager::focused_window()`. If focused window is Terminal or AegisPad, characters are inserted into the active text buffer.

---

## 4. Five Core Applications & Demo Suite Specification (R5)

### 4.1 Application 1: Crash-Test Demo App

#### 4.1.1 Purpose & Isolation Proof
The Crash-Test Demo App visually and mathematically proves hardware fault isolation. It runs as a Ring 3 userspace task. When any fault button is pressed, the CPU triggers a hardware exception in Ring 3. The kernel exception handler logs the fault, reaps the task, and returns control to the scheduler. The desktop environment, top menu bar, and other apps experience zero freezes or crashes.

#### 4.1.2 UI Layout Wireframe
```
+-------------------------------------------------------------+
| (*) ( ) ( )  Crash-Test Demo App                   [PID: 5] |
+-------------------------------------------------------------+
| AegisOS Ring 3 Hardware Isolation & Crash Recovery Proof    |
| Click any button below to trigger an intentional exception: |
|                                                             |
|  +-------------------------------------------------------+  |
|  |  [💥 Null Pointer Dereference]                        |  |
|  |  *(volatile u32*)0x0 = 0xDEADBEEF;  (#PF Vector 14)   |  |
|  +-------------------------------------------------------+  |
|                                                             |
|  +-------------------------------------------------------+  |
|  |  [➗ Divide by Zero]                                   |  |
|  |  let x = 100 / 0;                   (#DE Vector 0)    |  |
|  +-------------------------------------------------------+  |
|                                                             |
|  +-------------------------------------------------------+  |
|  |  [⛔ Out-of-Bounds Memory Write]                      |  |
|  |  *(0xFFFFFFFF80000000) = 0x1337;    (#GP/PF Ring 0)   |  |
|  +-------------------------------------------------------+  |
|                                                             |
|  +-------------------------------------------------------+  |
|  |  [🚫 Invalid Opcode]                                  |  |
|  |  asm!("ud2");                       (#UD Vector 6)    |  |
|  +-------------------------------------------------------+  |
+-------------------------------------------------------------+
```

#### 4.1.3 Fault Execution Mechanics & Handlers
1. **Null Pointer Dereference**:
   ```rust
   pub unsafe fn trigger_null_pointer() -> ! {
       let ptr = 0x0 as *mut u32;
       core::ptr::write_volatile(ptr, 0xDEAD_BEEF);
       loop {}
   }
   ```
2. **Divide-by-Zero**:
   ```rust
   pub unsafe fn trigger_divide_by_zero() -> ! {
       core::arch::asm!(
           "mov eax, 100",
           "xor ecx, ecx",
           "div ecx",
           options(noreturn)
       );
   }
   ```
3. **Out-of-Bounds Supervisor Write**:
   ```rust
   pub unsafe fn trigger_oob_write() -> ! {
       let kernel_ptr = 0xFFFF_FFFF_8000_0000 as *mut u32;
       core::ptr::write_volatile(kernel_ptr, 0xCAFE_BABE);
       loop {}
   }
   ```
4. **Invalid Opcode (`ud2`)**:
   ```rust
   pub unsafe fn trigger_invalid_opcode() -> ! {
       core::arch::asm!("ud2", options(noreturn));
   }
   ```

#### 4.1.4 Expected Kernel Recovery Behavior
- Ring 0 Interrupt Handler inspects `CS` selector in stack frame: `(frame.cs & 3) == 3` confirms Ring 3 fault.
- Serial Log Output:
  ```
  [FAULT] Ring 3 Exception #PF (Vector 14) in PID 5 (crashtest) at RIP 0x00000000004012A0
  [FAULT] Fault Address (CR2): 0x0000000000000000 | Error Code: 0x0002 (User, Write, Not-Present)
  [KERNEL] Terminating faulting task PID 5. Reclaiming 4 physical memory frames.
  [SCHED] Rescheduling next ready task. Desktop system intact.
  ```
- Result: Window Manager gracefully removes or marks the Crash-Test window as "Terminated by Kernel", while all other processes continue uninterrupted.

---

### 4.2 Application 2: Activity Monitor

#### 4.2.1 Purpose & Footprint Validation
Provides real-time telemetry of CPU load, memory distribution, and active task tables. Crucially verifies that total system memory at idle remains **< 60 MB**.

#### 4.2.2 UI Layout Wireframe
```
+---------------------------------------------------------------------------------+
| (*) ( ) ( )  Activity Monitor                                          [PID: 2] |
+---------------------------------------------------------------------------------+
|  CPU Utilization: 8.5%                   Memory Usage: 38.4 MB / 4096.0 MB      |
|  [||||                               ]   [|                                   ] |
|  +---------------------------------+     +------------------------------------+ |
|  | CPU Rolling History Graph (60s) |     | Total RAM:      4096.0 MB          | |
|  |     _/\_             _          |     | Used RAM:         38.4 MB (<60MB!) | |
|  | ___/    \___________/ \________ |     | Free RAM:       4057.6 MB          | |
|  +---------------------------------+     | Kernel Heap:       2.4 MB / 16 MB  | |
|                                          +------------------------------------+ |
|---------------------------------------------------------------------------------|
| Process Table:                                                   [Kill Process] |
| PID   | Process Name     | State    | Priority | Memory (KB) | CPU %            |
|-------|------------------|----------|----------|-------------|------------------|
| 0     | [idle]           | Ready    | 0 (Low)  | 64 KB       | 87.5%            |
| 1     | kernel_desktop   | Running  | 3 (High) | 14,336 KB   | 8.5%             |
| 2     | activity_monitor | Running  | 2 (Norm) | 2,148 KB    | 2.8%             |
| 3     | terminal_shell   | Blocked  | 2 (Norm) | 1,820 KB    | 0.0%             |
| 4     | aegis_pad        | Blocked  | 2 (Norm) | 1,512 KB    | 0.0%             |
| 5     | crash_test       | Ready    | 2 (Norm) | 1,200 KB    | 1.2%             |
+---------------------------------------------------------------------------------+
```

#### 4.2.3 Data Sources & Interactive Controls
- **CPU Calculation**: Derived from scheduler idle tick counter:
  $$\text{CPU\_Usage\%} = 100 \times \left(1.0 - \frac{\text{idle\_ticks\_delta}}{\text{total\_ticks\_delta}}\right)$$
- **Memory Calculation**: Queried from Physical Frame Allocator (`allocated_frames * 4096`) + Kernel Heap Allocator (`used_bytes`).
- **Interactive Process Termination**:
  - Selecting a row highlights it with `#007AFF`.
  - Clicking `[Kill Process]` issues `sys_kill(selected_pid)`.
  - Task is immediately removed from scheduler and physical memory is reclaimed in the graph.

---

### 4.3 Application 3: Interactive Terminal Shell

#### 4.3.1 Purpose & Command Interface
A full-featured virtual terminal emulator and interactive command line shell.

#### 4.3.2 UI Layout Wireframe
```
+-------------------------------------------------------------------+
| (*) ( ) ( )  Terminal — guest@aegis-os:~                 [PID: 3] |
+-------------------------------------------------------------------+
| AegisOS Virtual Terminal v1.0 (x86_64-unknown-none)               |
| Type 'help' to view available system commands.                    |
|                                                                   |
| aegis:~$ free                                                     |
| Physical Memory: Total 4096 MB | Used 38 MB | Free 4058 MB        |
| Kernel Heap:     Total 16384 KB | Used 2450 KB | Free 13934 KB    |
| Frame Allocator: 9830 / 1048576 frames in use                     |
|                                                                   |
| aegis:~$ ps                                                       |
| PID  NAME             STATE    MEMORY    CPU%                     |
| 0    [idle]           READY    64 KB     89%                      |
| 1    kernel_desktop   RUNNING  14 MB     7%                       |
| 2    activity_monitor RUNNING  2 MB      3%                       |
| 3    terminal_shell   RUNNING  1 MB      1%                       |
|                                                                   |
| aegis:~$ run crashtest                                            |
| [SYS] Spawned process 'crashtest' with PID 6                      |
|                                                                   |
| aegis:~$ _                                                        |
+-------------------------------------------------------------------+
```

#### 4.3.3 Supported Built-in CLI Commands

| Command | Syntax | Output & Operational Behavior |
|---|---|---|
| `help` | `help` | Lists all built-in commands with description and usage syntax. |
| `ps` | `ps` | Queries scheduler task list; outputs formatted table (`PID, NAME, STATE, MEMORY, CPU%`). |
| `kill` | `kill <pid>` | Issues `sys_kill` syscall for `<pid>`. Returns confirmation or error if PID not found. |
| `free` | `free` | Prints physical frame statistics, heap memory usage, and proves idle footprint < 60MB. |
| `echo` | `echo <text>` | Prints argument string back to console buffer. |
| `run` | `run <app>` | Launches specified application (`crashtest`, `monitor`, `pad`, `about`) as new task. |
| `clear` | `clear` | Flushes 80x25 character grid and resets cursor to `(0, 0)`. |
| `reboot` | `reboot` | Triggers system reboot via 8042 keyboard controller reset pulse (`0xFE` to port `0x64`). |

#### 4.3.4 Terminal Grid Buffer & Keyboard Navigation
- **Grid Geometry**: 8x16 font, 65 columns $\times$ 18 rows.
- **Buffer Capabilities**:
  - Backspace character deletion.
  - Multi-line buffer vertical scrolling when text exceeds row count.
  - Command History: Up/Down arrow keys recall previously executed shell commands.

---

### 4.4 Application 4: Text Editor (AegisPad)

#### 4.4.1 Purpose & Editing Features
A clean, lightweight text editor with line numbers, status bar, and multiline editing.

#### 4.4.2 UI Layout Wireframe
```
+-------------------------------------------------------------------+
| (*) ( ) ( )  AegisPad — welcome.txt                      [PID: 4] |
+-------------------------------------------------------------------+
| [ New ] [ Clear ] [ Sample Text ]                                 |
|-------------------------------------------------------------------|
|  1 | Welcome to AegisOS!                                          |
|  2 |                                                              |
|  3 | This operating system features:                              |
|  4 | - Ring 0 / Ring 3 hardware memory isolation                  |
|  5 | - Crash-resilient fault recovery                             |
|  6 | - macOS-inspired double-buffered desktop GUI                 |
|  7 | - Low memory footprint (< 60MB RAM)                          |
|  8 | _                                                            |
|    |                                                              |
|-------------------------------------------------------------------|
| Line: 8, Col: 1 | 215 characters | UTF-8 | AegisPad 1.0           |
+-------------------------------------------------------------------+
```

#### 4.4.3 Data Structures & Keybindings
```rust
pub struct AegisPad {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub filename: &'static str,
}
```
- **Operations**:
  - Printable ASCII characters: Inserted at `(cursor_row, cursor_col)`.
  - `Enter`: Splits current line into two, moves cursor to start of new line.
  - `Backspace`: Deletes character before cursor; joins lines if at column 0.
  - `Delete`: Deletes character at cursor position.
  - Arrow Keys: Navigates up, down, left, right across text buffer.

---

### 4.5 Application 5: About AegisOS Modal Dialog

#### 4.5.1 Purpose & Specifications Display
A polished macOS-style modal dialog showcasing OS branding, version, and architecture specifications.

#### 4.5.2 UI Layout Wireframe
```
+-------------------------------------------------------------+
| (*) ( ) ( )  About AegisOS                                  |
+-------------------------------------------------------------+
|                                                             |
|                       [ 🛡️ SHIELD LOGO ]                    |
|                            AegisOS                          |
|                         Version 1.0.0                       |
|                                                             |
|   +-----------------------------------------------------+   |
|   | Kernel:       Aegis Microkernel (Rust no_std)       |   |
|   | Bootloader:   Limine Boot Protocol v2               |   |
|   | Architecture: x86_64 Long Mode (Ring 0 / Ring 3)    |   |
|   | Memory:       4096 MB RAM (Active Footprint <60MB)  |   |
|   | Display:      1024x768x32 Linear Double-Buffered    |   |
|   | Toolchain:    Rust Nightly (x86_64-unknown-none)    |   |
|   +-----------------------------------------------------+   |
|                                                             |
|                         [   OK   ]                          |
+-------------------------------------------------------------+
```

---

## 5. Build, ISO Packaging & QEMU Execution Pipeline (R6)

### 5.1 Cargo Toolchain & Build Flags

#### 5.1.1 Target Specification
AegisOS compiles directly for bare-metal x86_64 using the target `x86_64-unknown-none`.

#### 5.1.2 `.cargo/config.toml`
```toml
[build]
target = "x86_64-unknown-none"

[target.x86_64-unknown-none]
rustflags = [
    "-C", "link-arg=-Tlinker.ld",
    "-C", "link-arg=-zmax-page-size=0x1000",
    "-C", "link-arg=-znoexecstack",
    "-C", "relocation-model=static",
    "-C", "code-model=kernel",
    "-C", "force-frame-pointers=yes",
]

[unstable]
build-std = ["core", "alloc", "compiler_builtins"]
build-std-features = ["compiler-builtins-mem"]
```

#### 5.1.3 `Cargo.toml`
```toml
[package]
name = "aegis_kernel"
version = "0.1.0"
edition = "2021"

[dependencies]
limine = "0.3"
spin = "0.9"
x86_64 = "0.14"
linked_list_allocator = "0.10"

[profile.release]
opt-level = 2
lto = true
panic = "abort"
codegen-units = 1

[profile.dev]
panic = "abort"
```

---

### 5.2 Higher-Half Linker Script (`linker.ld`)
The linker script maps the kernel to the Limine higher-half virtual address space `0xFFFFFFFF80100000`.

```ld
OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

PHDRS
{
    limine_requests PT_LOAD FLAGS(4); /* Read */
    text            PT_LOAD FLAGS(5); /* Read + Execute */
    rodata          PT_LOAD FLAGS(4); /* Read */
    data            PT_LOAD FLAGS(6); /* Read + Write */
}

SECTIONS
{
    . = 0xffffffff80100000;

    .limine_requests : {
        KEEP(*(.limine_requests_start))
        KEEP(*(.limine_requests))
        KEEP(*(.limine_requests_end))
    } :limine_requests

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    .text : {
        *(.text .text.*)
    } :text

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    .data : {
        *(.data .data.*)
    } :data

    .bss : {
        *(COMMON)
        *(.bss .bss.*)
    } :data

    /DISCARD/ : {
        *(.eh_frame)
        *(.note .note.*)
    }
}
```

---

### 5.3 Limine Bootloader Configuration (`limine.cfg`)
```ini
TIMEOUT=2
DEFAULT_ENTRY=1

/AegisOS (macOS-inspired Desktop GUI)
    PROTOCOL=limine
    KPATH=boot():/boot/aegis_kernel.elf
    RESOLUTION=1024x768x32
```

---

### 5.4 Hybrid BIOS + UEFI Bootable ISO Generation (`xorriso`)
AegisOS produces a hybrid ISO bootable on both legacy BIOS (El Torito) and modern UEFI firmware.

#### 5.4.1 ISO Directory Hierarchy
```
iso_root/
├── boot/
│   ├── aegis_kernel.elf
│   ├── limine-bios.sys
│   ├── limine-bios-cd.bin
│   └── limine-uefi-cd.bin
├── EFI/
│   └── BOOT/
│       ├── BOOTX64.EFI
│       └── BOOTIA32.EFI
└── limine.cfg
```

#### 5.4.2 ISO Creation Script Steps
```bash
# 1. Assemble ISO with xorriso
xorriso -as mkisofs -b boot/limine-bios-cd.bin \
    -no-emul-boot -boot-load-size 4 -boot-info-table \
    --efi-boot boot/limine-uefi-cd.bin \
    -efi-boot-part --efi-boot-image --protective-msdos-label \
    iso_root -o aegis_os.iso 2>/dev/null

# 2. Install Limine BIOS bootloader stage into ISO MBR
./limine/limine bios-install aegis_os.iso 2>/dev/null
```

---

### 5.5 Automated Launch Script (`run_qemu.sh`)
The automated runner builds the kernel, generates the ISO, and starts QEMU with standard display and serial debug redirection.

```bash
#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "[BUILD] Compiling AegisOS kernel in release mode..."
cargo build --release

echo "[PACKAGE] Assembling bootable hybrid ISO image..."
mkdir -p iso_root/boot iso_root/EFI/BOOT
cp target/x86_64-unknown-none/release/aegis_kernel iso_root/boot/aegis_kernel.elf
cp limine.cfg iso_root/
cp limine/limine-bios.sys limine/limine-bios-cd.bin limine/limine-uefi-cd.bin iso_root/boot/
cp limine/BOOTX64.EFI limine/BOOTIA32.EFI iso_root/EFI/BOOT/

xorriso -as mkisofs -b boot/limine-bios-cd.bin \
    -no-emul-boot -boot-load-size 4 -boot-info-table \
    --efi-boot boot/limine-uefi-cd.bin \
    -efi-boot-part --efi-boot-image --protective-msdos-label \
    iso_root -o aegis_os.iso 2>/dev/null

./limine/limine bios-install aegis_os.iso 2>/dev/null
echo "[PACKAGE] Successfully generated aegis_os.iso"

echo "[QEMU] Launching AegisOS in QEMU..."
qemu-system-x86_64 \
    -cdrom aegis_os.iso \
    -m 4G \
    -smp 1 \
    -vga std \
    -serial stdio \
    -no-reboot \
    -d guest_errors \
    "$@"
```

---

## 6. Features Discovered & Specification Catalog

### Features Discovered

| # | Category | Feature | Description | Inputs | Outputs | Error Behavior | Discovered Via |
|---|---|---|---|---|---|---|---|
| 1 | Graphics | Linear Framebuffer Request | Requests 32-bit linear RGB framebuffer from Limine | `FramebufferRequest` | `FramebufferResponse` with ptr, pitch, width, height | Fallback to default resolution | Limine Protocol Spec |
| 2 | Graphics | Double-Buffered Blitter | Offscreen 3.0MB backbuffer swapped to frontbuffer | Backbuffer scanlines | Frontbuffer VRAM write | Out-of-bounds clipped | R4 Specification |
| 3 | Graphics | 2D Vector Primitives | Anti-aliased rounded rects, circles, gradients | `(x, y, w, h, radius, color)` | Pixel buffer updates | Clamped to screen bounds | R4 Specification |
| 4 | Graphics | Alpha Blending Engine | Composites transparent layers (menu bar, dock) | `src: Color, dst: Color` | Blended `Color` | Alpha clamped 0..255 | R4 Specification |
| 5 | Graphics | 8x16 Embedded Font | Renders ASCII text from static VGA bitmap | `char, fg, bg, (x, y)` | 8x16 pixel block | Unknown chars mapped to `?` | Font Spec |
| 6 | Desktop | Top Menu Bar (24px) | Top status bar with Logo, Active App, Badges | Clock ticks, CPU%, RAM MB | Rendered 24px top bar | Text truncation on overflow | R4 Specification |
| 7 | Desktop | Memory Footprint Badge | Proves idle RAM consumption < 60 MB | Allocator telemetry | `[RAM: 38.4MB]` badge | Red warning if >= 60MB | ORIGINAL_REQUEST R4/R5 |
| 8 | Desktop | Floating Window Manager | Manages Z-ordered draggable application windows | Mouse drag, clicks | Positioned & focused windows | Window bounds clamped | R4 Specification |
| 9 | Desktop | Traffic-Light Buttons | Close (Red), Minimize (Yellow), Maximize (Green) | Mouse clicks on header buttons | Close task, minimize, zoom | Ignored if disabled | R4 Specification |
| 10| Desktop | Launcher Dock | Centered bottom dock with 5 clickable app icons | Mouse clicks on dock icons | Launch / bring to front | Ignored if max tasks reached | R4 Specification |
| 11| Input | PS/2 Mouse Decoder | Decodes 3-byte packets with 9-bit sign extension | Port 0x60 data bytes | `MousePacket (dx, dy, buttons)`| Resync on bit 3 mismatch | PS/2 Mouse Standard |
| 12| Input | Cursor Renderer | Draws 12x18 arrow cursor over backbuffer | `(cursor_x, cursor_y)` | Cursor overlay on screen | Clamped to 0..W-1, 0..H-1 | R4 Specification |
| 13| Input | PS/2 Keyboard Decoder | Decodes Set 1 scancodes to ASCII & KeyCodes | Port 0x60 scancodes | `GuiEvent::KeyDown(key)` | Unknown scancodes ignored | PS/2 Keyboard Standard |
| 14| Apps | Crash-Test Demo | Triggers #PF, #DE, #GP, #UD exceptions in Ring 3 | Button clicks | Task fault & reap, zero panic | Kernel reaps process cleanly | ORIGINAL_REQUEST R5.1 |
| 15| Apps | Activity Monitor | Live CPU%, RAM footprint graph, process table | Scheduler / Allocator data | Graphs and process rows | Highlight selected row | ORIGINAL_REQUEST R5.2 |
| 16| Apps | Process Termination | `[Kill Process]` button sends kill signal to PID | PID selection | Process terminated & freed | Error if PID 0/1 | R5.2 Specification |
| 17| Apps | Terminal Shell | Virtual CLI with `ps, kill, free, echo, run...` | Keyboard command string | Command output & scroll | "command not found" error | ORIGINAL_REQUEST R5.3 |
| 18| Apps | Text Editor (AegisPad) | Multi-line text buffer with cursor & status bar | Keystrokes | Text display & gutter numbers | Line length limit | ORIGINAL_REQUEST R5.4 |
| 19| Apps | About AegisOS Dialog | Modal dialog with shield logo & kernel specs | Click About in dock/menu | Modal window | Non-resizable dialog | ORIGINAL_REQUEST R5.5 |
| 20| Build | Hybrid ISO Creation | Generates BIOS + UEFI bootable ISO image | Kernel ELF, Limine binaries | `aegis_os.iso` | Error if xorriso fails | R6 Specification |
| 21| Build | QEMU Launch Runner | One-click script for build, ISO, and execution | `./run_qemu.sh` | Running QEMU instance | Returns exit code on fail | R6 Specification |

---

### Edge Cases & Observed / Specified Behaviors

| # | Feature | Input / Condition | Observed / Specified Behavior |
|---|---|---|---|
| 1 | Framebuffer Blitter | Screen dimension not multiple of 8/16 | Per-scanline copy safely copies exact byte width without page fault. |
| 2 | Window Dragging | Mouse dragged rapidly off-screen | Window coordinates clamped: titlebar remains visible within `(0..W-40, 24..H-30)`. |
| 3 | Traffic Light Close | Clicking Red button on Crash-Test | Cleanly closes window, unregisters task from scheduler, updates Activity Monitor. |
| 4 | Task Crash | Crash-Test clicking "Null Pointer" | CPU fires #PF in Ring 3, kernel logs exception to serial, reaps task, WM closes window, desktop stays 100% interactive. |
| 5 | Task Crash | Crash-Test clicking "Divide by Zero" | CPU fires #DE in Ring 3, kernel reaps task, RAM is freed, Activity Monitor shows memory drop. |
| 6 | Activity Monitor | Killing active application from table | Selected PID is removed, memory frames returned to frame allocator, graph updates immediately. |
| 7 | Terminal Shell | Executing `kill 9999` (non-existent PID) | Prints `Error: Process PID 9999 not found.` to terminal without crashing shell. |
| 8 | Terminal Shell | Executing `clear` | Resets terminal buffer lines, moves cursor to row 0 col 0, redraws clean canvas. |
| 9 | Terminal Shell | Executing `reboot` | Writes `0xFE` to port `0x64`, immediately causing x86 CPU reset. |
| 10| Text Editor | Backspace at start of line 2 | Merges line 2 into line 1 at previous line's end column. |
| 11| PS/2 Mouse Stream | Out-of-sync byte received (bit 3 == 0) | Discards byte and resets packet state machine to byte 0 to prevent cursor jumps. |
| 12| Memory Constraint | OS booted with 4GB RAM | Idle memory usage remains < 60MB RAM (~38.4MB used), verified by Activity Monitor & Menu Bar. |
