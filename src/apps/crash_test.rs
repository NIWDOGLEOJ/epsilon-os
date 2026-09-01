//! Crash-Test Demo Application for AegisOS
//!
//! Provides interactive UI buttons to intentionally trigger Ring 3 hardware
//! exceptions (#PF Null Pointer, #DE Divide-by-Zero, #GP/#PF Out-of-Bounds Write,
//! and #UD Invalid Opcode) to visually and mathematically prove that process crashes
//! terminate only the offending task while the kernel and desktop continue running.

use alloc::string::{String, ToString};

use crate::drivers::framebuffer::Framebuffer;
use crate::gui::font::draw_string;
use crate::gui::primitives::{draw_rect_outline, draw_rounded_rect, Color, Rect};
use crate::gui::window::Window;

// ============================================================================
// Fault Trigger Hardware Routines
// ============================================================================

/// Triggers a Page Fault (#PF, Vector 14) via null pointer dereference write.
pub unsafe fn trigger_null_pointer() -> ! {
    let ptr = 0x0 as *mut u32;
    core::ptr::write_volatile(ptr, 0xDEAD_BEEF);
    loop {
        core::hint::spin_loop();
    }
}

/// Triggers a Divide-by-Zero exception (#DE, Vector 0).
pub unsafe fn trigger_divide_by_zero() -> ! {
    core::arch::asm!(
        "mov eax, 100",
        "xor ecx, ecx",
        "div ecx",
        options(noreturn)
    );
}

/// Triggers a Supervisor Address Page Fault / General Protection Fault.
pub unsafe fn trigger_oob_write() -> ! {
    let kernel_ptr = 0xFFFF_FFFF_8000_0000 as *mut u32;
    core::ptr::write_volatile(kernel_ptr, 0xCAFE_BABE);
    loop {
        core::hint::spin_loop();
    }
}

/// Triggers an Invalid Opcode exception (#UD, Vector 6).
pub unsafe fn trigger_invalid_opcode() -> ! {
    core::arch::asm!("ud2", options(noreturn));
}

// ============================================================================
// Crash-Test Application UI & Controller
// ============================================================================

pub struct CrashTestApp {
    pub pid: Option<u64>,
    pub status_msg: String,
}

impl CrashTestApp {
    pub fn new(pid: Option<u64>) -> Self {
        Self {
            pid,
            status_msg: "Ready. Click any button to test hardware isolation.".to_string(),
        }
    }

    /// Renders the Crash-Test application inside the window client area.
    pub fn render(&self, win: &Window, fb: &mut Framebuffer) {
        let client = win.client_rect();
        if client.width < 300 || client.height < 200 {
            return;
        }

        // Header Title & Description
        draw_string(
            fb,
            client.x + 12,
            client.y + 10,
            "AegisOS Ring 3 Hardware Isolation & Crash Recovery Proof",
            Color::WHITE,
            None,
        );
        draw_string(
            fb,
            client.x + 12,
            client.y + 28,
            "Click any button below to trigger an intentional exception:",
            Color::TEXT_DIM,
            None,
        );

        // 4 Fault Buttons
        let btn_w = client.width.saturating_sub(24);
        let btn_h = 42;
        let start_y = client.y + 50;

        let buttons = [
            (
                "[ Null Pointer Dereference ]",
                "*(volatile u32*)0x0 = 0xDEADBEEF;  (#PF Vector 14)",
                Color::rgb(255, 95, 86),
            ),
            (
                "[ Divide by Zero ]",
                "let x = 100 / 0;                   (#DE Vector 0)",
                Color::rgb(255, 189, 46),
            ),
            (
                "[ Out-of-Bounds Memory Write ]",
                "*(0xFFFFFFFF80000000) = 0x1337;    (#GP/PF Ring 0)",
                Color::rgb(255, 120, 100),
            ),
            (
                "[ Invalid Opcode ]",
                "asm!(\"ud2\");                       (#UD Vector 6)",
                Color::rgb(200, 100, 255),
            ),
        ];

        for (i, &(btn_title, btn_desc, accent)) in buttons.iter().enumerate() {
            let by = start_y + (i as i32 * (btn_h as i32 + 8));
            let btn_rect = Rect::new(client.x + 12, by, btn_w, btn_h);

            draw_rounded_rect(fb, btn_rect, 6, Color::BUTTON_BG);
            draw_rect_outline(fb, btn_rect, Color::BUTTON_BORDER, 1);

            // Left accent bar
            draw_rounded_rect(
                fb,
                Rect::new(client.x + 12, by, 4, btn_h),
                2,
                accent,
            );

            // Button Title & Description
            draw_string(fb, client.x + 24, by + 6, btn_title, Color::WHITE, None);
            draw_string(fb, client.x + 24, by + 22, btn_desc, Color::TEXT_DIM, None);
        }

        // Status banner, placed directly below the last button.
        //
        // It used to be anchored to `client.height - 24`, which for the 520x270
        // window put it inside the fourth button rather than under it.
        let buttons_bottom = start_y + (buttons.len() as i32 * (btn_h as i32 + 8));
        let status_y = buttons_bottom + 4;
        draw_string(
            fb,
            client.x + 12,
            status_y,
            &self.status_msg,
            Color::TEXT_HIGHLIGHT,
            None,
        );
    }

    /// Handles click on client area; returns true if a fault button was triggered.
    pub fn handle_click(&mut self, win: &Window, px: i32, py: i32) -> Option<usize> {
        let client = win.client_rect();
        let btn_w = client.width.saturating_sub(24);
        let btn_h = 42;
        let start_y = client.y + 50;

        for i in 0..4 {
            let by = start_y + (i as i32 * (btn_h as i32 + 8));
            let btn_rect = Rect::new(client.x + 12, by, btn_w, btn_h);
            if btn_rect.contains(px, py) {
                self.status_msg = match i {
                    0 => "[FAULT] Triggered #PF Null Pointer Dereference!".to_string(),
                    1 => "[FAULT] Triggered #DE Divide by Zero!".to_string(),
                    2 => "[FAULT] Triggered #GP/#PF Out-of-Bounds Write!".to_string(),
                    3 => "[FAULT] Triggered #UD Invalid Opcode (ud2)!".to_string(),
                    _ => "Fault triggered.".to_string(),
                };
                return Some(i);
            }
        }
        None
    }
}
