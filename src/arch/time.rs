//! Hardware TSC Timing, Calibration, and 60 FPS Compositor Frame Pacer
//!
//! Calibrates the CPU Time Stamp Counter (TSC) against the 100 Hz PIT timer
//! using low-power hardware halt (hlt) synchronization, establishing an exact
//! 60 FPS (16.667 ms) budget for smooth tear-free desktop rendering.

use core::arch::x86_64::_rdtsc;
use crate::serial_println;

/// 60 FPS Target Frame Rate
pub const TARGET_FPS: u32 = 60;

/// Compositor Frame Pacing Controller
pub struct FramePacer {
    target_cycles_per_frame: u64,
    last_frame_tsc: u64,
    frame_count: u32,
    last_fps_update_tsc: u64,
    current_fps: u32,
    cpu_mhz: u64,
}

impl FramePacer {
    /// Calibrates the TSC against the 100 Hz PIT timer over a 30 ms window (3 ticks).
    pub fn init() -> Self {
        // 1. Wait for next clean tick boundary using hlt
        let initial_tick = crate::task::get_uptime_ticks();
        while crate::task::get_uptime_ticks() == initial_tick {
            unsafe {
                core::arch::asm!("sti; hlt", options(nomem, nostack));
            }
        }

        // 2. Sample TSC at start of tick window
        let start_tick = crate::task::get_uptime_ticks();
        let tsc_start = unsafe { _rdtsc() };

        // Wait for 3 ticks (30 ms @ 100 Hz PIT)
        let target_tick = start_tick + 3;
        while crate::task::get_uptime_ticks() < target_tick {
            unsafe {
                core::arch::asm!("sti; hlt", options(nomem, nostack));
            }
        }
        let tsc_end = unsafe { _rdtsc() };

        let cycles_30ms = tsc_end.saturating_sub(tsc_start);
        // (cycles / 30ms) * 1000ms = cycles * 100 / 3
        let raw_cycles_per_sec = (cycles_30ms * 100) / 3;

        // Ensure a healthy CPU frequency (> 100 MHz, fallback to 2.5 GHz)
        let cycles_per_sec = if raw_cycles_per_sec > 100_000_000 {
            raw_cycles_per_sec
        } else {
            2_500_000_000
        };

        let cpu_mhz = cycles_per_sec / 1_000_000;
        let target_cycles_per_frame = cycles_per_sec / TARGET_FPS as u64;

        serial_println!(
            "[TIME] Calibrated TSC: ~{} MHz | Target 60 FPS: {} cycles/frame (16.67 ms)",
            cpu_mhz,
            target_cycles_per_frame
        );

        let now = unsafe { _rdtsc() };
        Self {
            target_cycles_per_frame,
            last_frame_tsc: now,
            frame_count: 0,
            last_fps_update_tsc: now,
            current_fps: 60,
            cpu_mhz,
        }
    }

    /// Delays until the 16.667 ms frame deadline has elapsed.
    ///
    /// If the frame took longer than 16.667 ms to render, yields immediately
    /// without stalling so the pipeline runs at maximum available speed.
    #[inline(always)]
    pub fn pace_frame(&mut self) {
        let now = unsafe { _rdtsc() };
        let elapsed = now.saturating_sub(self.last_frame_tsc);

        if elapsed < self.target_cycles_per_frame {
            let target = self.last_frame_tsc + self.target_cycles_per_frame;
            while unsafe { _rdtsc() } < target {
                core::hint::spin_loop();
            }
        }

        let frame_end = unsafe { _rdtsc() };
        self.last_frame_tsc = frame_end;
        self.frame_count = self.frame_count.wrapping_add(1);

        // Update rolling FPS calculation every 1 second
        let cycles_since_fps = frame_end.saturating_sub(self.last_fps_update_tsc);
        let one_sec_cycles = self.cpu_mhz * 1_000_000;
        if cycles_since_fps >= one_sec_cycles && one_sec_cycles > 0 {
            self.current_fps = self.frame_count;
            self.frame_count = 0;
            self.last_fps_update_tsc = frame_end;
        }
    }

    /// Returns the current measured frames per second.
    #[inline(always)]
    pub fn get_fps(&self) -> u32 {
        self.current_fps
    }
}
