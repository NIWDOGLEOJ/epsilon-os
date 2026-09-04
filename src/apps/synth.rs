//! AegisSynth — Interactive Chiptune Synthesizer & 16-Step Piano Roll Studio for AegisOS
//!
//! Features an interactive 2-octave chromatic piano keyboard (C4 to B5),
//! playable via mouse and PC keyboard, a 4-track 16-step pattern sequencer / tracker,
//! tempo controls (BPM), iconic chiptune presets, real-time scanning playhead,
//! and direct hardware PC speaker audio synthesis.

use alloc::format;
use alloc::string::{String, ToString};

use crate::drivers::framebuffer::Framebuffer;
use crate::drivers::ps2_keyboard::{KeyCode, KeyEvent};
use crate::gui::font::draw_string;
use crate::gui::primitives::{
    draw_line, draw_rect, draw_rounded_rect, draw_rounded_rect_outline, Color, Rect,
};
use crate::gui::window::Window;

/// Note frequency definitions in Hertz.
pub const NOTE_C4: u32 = 262;
pub const NOTE_CS4: u32 = 277;
pub const NOTE_D4: u32 = 294;
pub const NOTE_DS4: u32 = 311;
pub const NOTE_E4: u32 = 330;
pub const NOTE_F4: u32 = 349;
pub const NOTE_FS4: u32 = 370;
pub const NOTE_G4: u32 = 392;
pub const NOTE_GS4: u32 = 415;
pub const NOTE_A4: u32 = 440;
pub const NOTE_AS4: u32 = 466;
pub const NOTE_B4: u32 = 494;

pub const NOTE_C5: u32 = 523;
pub const NOTE_CS5: u32 = 554;
pub const NOTE_D5: u32 = 587;
pub const NOTE_DS5: u32 = 622;
pub const NOTE_E5: u32 = 659;
pub const NOTE_F5: u32 = 698;
pub const NOTE_FS5: u32 = 740;
pub const NOTE_G5: u32 = 784;
pub const NOTE_GS5: u32 = 831;
pub const NOTE_A5: u32 = 880;
pub const NOTE_AS5: u32 = 932;
pub const NOTE_B5: u32 = 988;

/// White piano keys (14 keys across 2 octaves)
pub const WHITE_KEYS: [(u32, &str, &str); 14] = [
    (NOTE_C4, "C4", "A"),
    (NOTE_D4, "D4", "S"),
    (NOTE_E4, "E4", "D"),
    (NOTE_F4, "F4", "F"),
    (NOTE_G4, "G4", "G"),
    (NOTE_A4, "A4", "H"),
    (NOTE_B4, "B4", "J"),
    (NOTE_C5, "C5", "K"),
    (NOTE_D5, "D5", "L"),
    (NOTE_E5, "E5", ";"),
    (NOTE_F5, "F5", ""),
    (NOTE_G5, "G5", ""),
    (NOTE_A5, "A5", ""),
    (NOTE_B5, "B5", ""),
];

/// Black piano keys with relative white key offsets
pub const BLACK_KEYS: [(u32, &str, &str, usize); 10] = [
    (NOTE_CS4, "C#4", "W", 0),
    (NOTE_DS4, "D#4", "E", 1),
    (NOTE_FS4, "F#4", "T", 3),
    (NOTE_GS4, "G#4", "Y", 4),
    (NOTE_AS4, "A#4", "U", 5),
    (NOTE_CS5, "C#5", "O", 7),
    (NOTE_DS5, "D#5", "P", 8),
    (NOTE_FS5, "F#5", "", 10),
    (NOTE_GS5, "G#5", "", 11),
    (NOTE_AS5, "A#5", "", 12),
];

pub struct SynthApp {
    pub is_playing: bool,
    pub current_step: usize,
    pub bpm: u32,
    pub last_tick: u64,
    pub active_key: Option<u32>, // frequency of currently pressed/played key
    pub key_decay_frames: u8,
    // 4 Tracks x 16 Steps
    // Track 0: Lead Melody (High)
    // Track 1: Arpeggio (Mid)
    // Track 2: Bassline (Low)
    // Track 3: Chiptune Percussion (Beat)
    pub pattern: [[bool; 16]; 4],
    pub preset_idx: usize,
    pub status_message: Option<String>,
}

impl SynthApp {
    pub fn new() -> Self {
        let mut app = Self {
            is_playing: false,
            current_step: 0,
            bpm: 120,
            last_tick: 0,
            active_key: None,
            key_decay_frames: 0,
            pattern: [[false; 16]; 4],
            preset_idx: 0,
            status_message: Some("AegisSynth Ready".to_string()),
        };
        app.load_preset(0);
        app
    }

    /// Loads a built-in chiptune pattern preset.
    pub fn load_preset(&mut self, idx: usize) {
        self.pattern = [[false; 16]; 4];
        self.preset_idx = idx % 3;

        match self.preset_idx {
            0 => {
                // Preset 0: "Cyberpunk Arp"
                // Track 0 (Lead):
                for &s in &[0, 3, 6, 8, 11, 14] {
                    self.pattern[0][s] = true;
                }
                // Track 1 (Arp):
                for &s in &[1, 4, 7, 9, 12, 15] {
                    self.pattern[1][s] = true;
                }
                // Track 2 (Bass):
                for &s in &[0, 4, 8, 12] {
                    self.pattern[2][s] = true;
                }
                // Track 3 (Beat):
                for &s in &[0, 2, 4, 6, 8, 10, 12, 14] {
                    self.pattern[3][s] = true;
                }
                self.status_message = Some("Loaded: Cyberpunk Arp".to_string());
            }
            1 => {
                // Preset 1: "8-Bit Mario"
                // Track 0 (Lead):
                for &s in &[0, 2, 4, 7, 10, 12] {
                    self.pattern[0][s] = true;
                }
                // Track 1 (Harmony):
                for &s in &[1, 3, 5, 8, 11, 13] {
                    self.pattern[1][s] = true;
                }
                // Track 2 (Bass):
                for &s in &[0, 6, 8, 14] {
                    self.pattern[2][s] = true;
                }
                // Track 3 (Beat):
                for &s in &[2, 6, 10, 14] {
                    self.pattern[3][s] = true;
                }
                self.status_message = Some("Loaded: 8-Bit Mario".to_string());
            }
            2 => {
                // Preset 2: "Retro Bassline"
                // Track 2 (Heavy Bass):
                for s in 0..16 {
                    if s % 2 == 0 {
                        self.pattern[2][s] = true;
                    }
                }
                // Track 0 (Lead flourishes):
                for &s in &[4, 7, 12, 15] {
                    self.pattern[0][s] = true;
                }
                // Track 3 (Kick/Snare):
                for &s in &[0, 4, 8, 12] {
                    self.pattern[3][s] = true;
                }
                self.status_message = Some("Loaded: Retro Bassline".to_string());
            }
            _ => {}
        }
    }

    /// Clears all tracks in the sequencer pattern.
    pub fn clear_pattern(&mut self) {
        self.pattern = [[false; 16]; 4];
        self.current_step = 0;
        self.status_message = Some("Pattern Cleared".to_string());
    }

    /// Toggles a specific step trigger on a track.
    pub fn toggle_step(&mut self, track: usize, step: usize) {
        if track < 4 && step < 16 {
            self.pattern[track][step] = !self.pattern[track][step];
            // Auditory feedback: play note for this track
            let freq = Self::track_frequency(track, step);
            self.play_tone_direct(freq, 3);
        }
    }

    /// Returns the tone frequency associated with a track and step.
    pub fn track_frequency(track: usize, step: usize) -> u32 {
        match track {
            0 => {
                // Lead melody: Pentatonic high scale
                match step % 8 {
                    0 => NOTE_C5,
                    1 => NOTE_D5,
                    2 => NOTE_E5,
                    3 => NOTE_G5,
                    4 => NOTE_A5,
                    5 => NOTE_C5 * 2, // C6
                    6 => NOTE_A5,
                    _ => NOTE_G5,
                }
            }
            1 => {
                // Arpeggio: Mid harmonic scale
                match (step + 2) % 6 {
                    0 => NOTE_E4,
                    1 => NOTE_G4,
                    2 => NOTE_B4,
                    3 => NOTE_C5,
                    4 => NOTE_E5,
                    _ => NOTE_G4,
                }
            }
            2 => {
                // Bassline: Low octaves
                match step % 4 {
                    0 => 131, // C3
                    1 => 165, // E3
                    2 => 196, // G3
                    _ => 220, // A3
                }
            }
            3 => {
                // Percussion: Short snappy bursts
                if step % 4 == 0 {
                    110 // Kick low pop
                } else if step % 2 == 0 {
                    988 // Snare high tick
                } else {
                    440 // Hi-hat blip
                }
            }
            _ => NOTE_C4,
        }
    }

    /// Plays a tone directly with visual active key depression.
    pub fn play_tone_direct(&mut self, freq: u32, decay_frames: u8) {
        self.active_key = Some(freq);
        self.key_decay_frames = decay_frames;
        crate::drivers::speaker::beep(freq, 60);
    }

    /// Advances the pattern sequencer based on 100Hz hardware timer ticks.
    pub fn tick_sequencer(&mut self, current_uptime_ticks: u64) {
        // Handle visual key decay
        if self.key_decay_frames > 0 {
            self.key_decay_frames -= 1;
            if self.key_decay_frames == 0 {
                self.active_key = None;
            }
        }

        if !self.is_playing {
            return;
        }

        // BPM calculation:
        // Steps per minute = BPM * 4 (16th notes)
        // Steps per second = (BPM * 4) / 60
        // Ticks per step at 100 Hz = 100 / ((BPM * 4) / 60) = 6000 / (BPM * 4) = 1500 / BPM
        let ticks_per_step = (1500 / self.bpm.max(40)).max(2) as u64;

        if current_uptime_ticks >= self.last_tick.wrapping_add(ticks_per_step) {
            self.last_tick = current_uptime_ticks;
            self.current_step = (self.current_step + 1) % 16;

            // Trigger notes for the active step
            let step = self.current_step;
            let mut triggered_note: Option<u32> = None;

            // Priority: Lead > Arp > Bass > Beat
            for track in 0..4 {
                if self.pattern[track][step] {
                    triggered_note = Some(Self::track_frequency(track, step));
                    break;
                }
            }

            if let Some(freq) = triggered_note {
                self.play_tone_direct(freq, 4);
            }
        }
    }

    /// Handles keyboard events when AegisSynth has active focus.
    pub fn handle_key(&mut self, event: KeyEvent) {
        if !event.pressed {
            return;
        }

        // Space bar: Toggle Play / Stop
        if event.char_byte == Some(b' ') || event.code == KeyCode::Printable(b' ') {
            self.is_playing = !self.is_playing;
            let state = if self.is_playing { "Playing" } else { "Stopped" };
            self.status_message = Some(format!("Sequencer {}", state));
            return;
        }

        // Piano Keyboard Direct Playing:
        let played_freq = match event.char_byte {
            Some(b'a') | Some(b'A') => Some(NOTE_C4),
            Some(b'w') | Some(b'W') => Some(NOTE_CS4),
            Some(b's') | Some(b'S') => Some(NOTE_D4),
            Some(b'e') | Some(b'E') => Some(NOTE_DS4),
            Some(b'd') | Some(b'D') => Some(NOTE_E4),
            Some(b'f') | Some(b'F') => Some(NOTE_F4),
            Some(b't') | Some(b'T') => Some(NOTE_FS4),
            Some(b'g') | Some(b'G') => Some(NOTE_G4),
            Some(b'y') | Some(b'Y') => Some(NOTE_GS4),
            Some(b'h') | Some(b'H') => Some(NOTE_A4),
            Some(b'u') | Some(b'U') => Some(NOTE_AS4),
            Some(b'j') | Some(b'J') => Some(NOTE_B4),
            Some(b'k') | Some(b'K') => Some(NOTE_C5),
            Some(b'o') | Some(b'O') => Some(NOTE_CS5),
            Some(b'l') | Some(b'L') => Some(NOTE_D5),
            Some(b'p') | Some(b'P') => Some(NOTE_DS5),
            Some(b';') => Some(NOTE_E5),
            _ => None,
        };

        if let Some(freq) = played_freq {
            self.play_tone_direct(freq, 6);
        }
    }

    /// Handles mouse clicks on transport controls, sequencer matrix, or piano keys.
    pub fn handle_mouse_down(&mut self, win: &Window, x: i32, y: i32) {
        let client = win.client_rect();

        // ── 1. Top Transport Controls (y = client.y .. client.y + 36) ──
        if y >= client.y && y < client.y + 36 {
            // [ ▶ Play ] / [ ■ Stop ] at x = client.x + 8 .. + 76
            if x >= client.x + 8 && x <= client.x + 76 {
                self.is_playing = !self.is_playing;
                let state = if self.is_playing { "Playing" } else { "Stopped" };
                self.status_message = Some(format!("Sequencer {}", state));
                return;
            }

            // [ Preset ] at x = client.x + 84 .. + 174
            if x >= client.x + 84 && x <= client.x + 174 {
                let next_idx = (self.preset_idx + 1) % 3;
                self.load_preset(next_idx);
                return;
            }

            // [ Clear ] at x = client.x + 182 .. + 242
            if x >= client.x + 182 && x <= client.x + 242 {
                self.clear_pattern();
                return;
            }

            // BPM [ - ] at x = client.x + 250 .. + 276
            if x >= client.x + 250 && x <= client.x + 276 {
                self.bpm = self.bpm.saturating_sub(10).max(60);
                self.status_message = Some(format!("Tempo: {} BPM", self.bpm));
                return;
            }

            // BPM [ + ] at x = client.x + 350 .. + 376
            if x >= client.x + 350 && x <= client.x + 376 {
                self.bpm = (self.bpm + 10).min(240);
                self.status_message = Some(format!("Tempo: {} BPM", self.bpm));
                return;
            }
        }

        // ── 2. 16-Step Pattern Sequencer Grid (y = client.y + 38 .. client.y + 174) ──
        let grid_y = client.y + 38;
        if y >= grid_y && y < grid_y + 136 {
            let row_h = 32i32;
            let track = ((y - grid_y) / row_h).clamp(0, 3) as usize;

            // Track buttons start at x = client.x + 80
            let start_x = client.x + 80;
            let step_w = 26i32;
            if x >= start_x && x < start_x + (16 * step_w) {
                let step = ((x - start_x) / step_w).clamp(0, 15) as usize;
                self.toggle_step(track, step);
                return;
            }
        }

        // ── 3. Interactive Piano Keyboard (y = client.y + 182 .. client.bottom() - 20) ──
        let piano_y = client.y + 182;
        let piano_h = 135i32;
        let white_w = 35i32;
        let black_w = 22i32;
        let black_h = 80i32;

        if y >= piano_y && y < piano_y + piano_h {
            // Check Black Keys First (higher Z-order)
            if y < piano_y + black_h {
                for (freq, _, _, white_idx) in BLACK_KEYS {
                    let bx = client.x + 14 + (white_idx as i32 * white_w) + (white_w - black_w / 2);
                    if x >= bx && x < bx + black_w {
                        self.play_tone_direct(freq, 8);
                        return;
                    }
                }
            }

            // Check White Keys
            let start_wx = client.x + 14;
            if x >= start_wx && x < start_wx + (14 * white_w) {
                let white_idx = ((x - start_wx) / white_w).clamp(0, 13) as usize;
                let (freq, _, _) = WHITE_KEYS[white_idx];
                self.play_tone_direct(freq, 8);
                return;
            }
        }
    }

    /// Renders AegisSynth inside the window client area.
    pub fn render(&mut self, win: &Window, fb: &mut Framebuffer) {
        let client = win.client_rect();
        if client.width < 400 || client.height < 300 {
            return;
        }

        // Dark Studio Synth Background
        draw_rect(fb, client, Color::rgb(18, 20, 26));

        // ── 1. Top Transport Header Bar (36px) ──
        let bar_rect = Rect::new(client.x, client.y, client.width, 36);
        draw_rect(fb, bar_rect, Color::rgb(26, 30, 38));
        draw_rect(fb, Rect::new(client.x, client.y + 35, client.width, 1), Color::WINDOW_BORDER);

        // [ ▶ Play ] / [ ■ Stop ] Button
        let play_color = if self.is_playing {
            Color::rgb(220, 50, 50)
        } else {
            Color::rgb(40, 180, 80)
        };
        let play_text = if self.is_playing { "■ Stop" } else { "▶ Play" };
        let play_btn_rect = Rect::new(client.x + 8, client.y + 6, 68, 24);
        draw_rounded_rect(fb, play_btn_rect, 4, play_color);
        draw_string(fb, client.x + 16, client.y + 10, play_text, Color::WHITE, None);

        // [ Preset: ... ]
        let preset_btn_rect = Rect::new(client.x + 84, client.y + 6, 90, 24);
        draw_rounded_rect(fb, preset_btn_rect, 4, Color::BUTTON_BG);
        let preset_name = match self.preset_idx {
            0 => "P:Cyber",
            1 => "P:Mario",
            _ => "P:Bass",
        };
        draw_string(fb, client.x + 92, client.y + 10, preset_name, Color::WHITE, None);

        // [ Clear ]
        let clear_btn_rect = Rect::new(client.x + 182, client.y + 6, 60, 24);
        draw_rounded_rect(fb, clear_btn_rect, 4, Color::BUTTON_BG);
        draw_string(fb, client.x + 192, client.y + 10, "Clear", Color::WHITE, None);

        // Tempo / BPM Controls
        let bpm_str = format!("{} BPM", self.bpm);
        draw_rounded_rect(fb, Rect::new(client.x + 250, client.y + 6, 26, 24), 3, Color::BUTTON_BG);
        draw_string(fb, client.x + 258, client.y + 10, "-", Color::WHITE, None);

        draw_string(fb, client.x + 284, client.y + 10, &bpm_str, Color::rgb(255, 200, 60), None);

        draw_rounded_rect(fb, Rect::new(client.x + 350, client.y + 6, 26, 24), 3, Color::BUTTON_BG);
        draw_string(fb, client.x + 358, client.y + 10, "+", Color::WHITE, None);

        // ── 2. 16-Step Pattern Sequencer / Tracker Studio ──
        let track_names = ["LEAD", "ARPG", "BASS", "BEAT"];
        let track_colors = [
            Color::rgb(0, 230, 255),   // Cyan Lead
            Color::rgb(255, 175, 40),  // Amber Arp
            Color::rgb(255, 70, 180),  // Magenta Bass
            Color::rgb(100, 255, 120), // Lime Beat
        ];

        let grid_y = client.y + 42;
        let row_h = 32i32;
        let start_x = client.x + 78;
        let step_w = 26i32;

        for track in 0..4 {
            let ry = grid_y + (track as i32 * row_h);

            // Track Label Badge
            let label_rect = Rect::new(client.x + 8, ry + 2, 62, 26);
            draw_rounded_rect(fb, label_rect, 3, Color::rgb(30, 34, 44));
            draw_string(fb, client.x + 14, ry + 8, track_names[track], track_colors[track], None);

            // 16 Step Buttons
            for step in 0..16 {
                let sx = start_x + (step as i32 * step_w);
                let btn_rect = Rect::new(sx + 2, ry + 3, step_w as u32 - 4, 24);

                let is_on = self.pattern[track][step];
                let is_active_step = self.is_playing && self.current_step == step;

                // Distinct color for beat groupings (every 4 steps)
                let base_step_color = if (step / 4) % 2 == 0 {
                    Color::rgb(28, 32, 42)
                } else {
                    Color::rgb(22, 25, 34)
                };

                let fill_color = if is_on {
                    if is_active_step {
                        Color::WHITE // Active firing note flash
                    } else {
                        track_colors[track]
                    }
                } else if is_active_step {
                    Color::rgb(55, 65, 85) // Playhead scan on empty step
                } else {
                    base_step_color
                };

                draw_rounded_rect(fb, btn_rect, 2, fill_color);
                draw_rounded_rect_outline(fb, btn_rect, 2, Color::rgb(50, 56, 72));
            }
        }

        // Animated Playhead Line
        if self.is_playing {
            let ph_x = start_x + (self.current_step as i32 * step_w) + (step_w / 2);
            draw_line(fb, ph_x, grid_y, ph_x, grid_y + (4 * row_h) - 4, Color::rgb(255, 220, 50));
        }

        // ── 3. Interactive 2-Octave Chromatic Piano Keyboard ──
        let piano_y = client.y + 182;
        let piano_h = 135i32;
        let white_w = 35i32;
        let black_w = 22i32;
        let black_h = 80i32;
        let start_wx = client.x + 14;

        // Draw White Keys
        for (i, &(freq, note_name, key_char)) in WHITE_KEYS.iter().enumerate() {
            let kx = start_wx + (i as i32 * white_w);
            let k_rect = Rect::new(kx, piano_y, white_w as u32 - 1, piano_h as u32);

            let is_pressed = self.active_key == Some(freq);
            let key_color = if is_pressed {
                Color::rgb(80, 200, 255) // Cyan depressed glow
            } else {
                Color::rgb(245, 245, 245) // Ivory white
            };

            draw_rounded_rect(fb, k_rect, 3, key_color);
            draw_rounded_rect_outline(fb, k_rect, 3, Color::rgb(180, 180, 180));

            // Note Name label at bottom
            let text_color = if is_pressed { Color::WHITE } else { Color::rgb(40, 40, 40) };
            draw_string(fb, kx + 4, piano_y + piano_h - 32, note_name, text_color, None);

            // Keyboard shortcut char
            if !key_char.is_empty() {
                draw_string(fb, kx + 12, piano_y + piano_h - 16, key_char, Color::rgb(120, 120, 120), None);
            }
        }

        // Draw Black Keys (Overlaid on top)
        for &(freq, _, key_char, white_idx) in BLACK_KEYS.iter() {
            let bx = start_wx + (white_idx as i32 * white_w) + (white_w - black_w / 2);
            let b_rect = Rect::new(bx, piano_y, black_w as u32, black_h as u32);

            let is_pressed = self.active_key == Some(freq);
            let key_color = if is_pressed {
                Color::rgb(255, 140, 40) // Orange depressed glow
            } else {
                Color::rgb(28, 28, 32) // Ebony black
            };

            draw_rounded_rect(fb, b_rect, 3, key_color);
            draw_rounded_rect_outline(fb, b_rect, 3, Color::rgb(60, 60, 65));

            // Keyboard shortcut char on black key
            if !key_char.is_empty() {
                draw_string(fb, bx + 6, piano_y + black_h - 18, key_char, Color::rgb(200, 200, 200), None);
            }
        }

        // ── 4. Bottom Telemetry Bar (20px) ──
        let status_y = client.bottom() - 20;
        let status_rect = Rect::new(client.x, status_y, client.width, 20);
        draw_rect(fb, status_rect, Color::rgb(22, 25, 32));
        draw_rect(fb, Rect::new(client.x, status_y, client.width, 1), Color::WINDOW_BORDER);

        let msg = self.status_message.as_deref().unwrap_or("AegisSynth Ready");
        let active_freq_str = if let Some(f) = self.active_key {
            format!("Tone: {} Hz", f)
        } else {
            "Tone: Idle".to_string()
        };
        let status_text = format!(
            "Step: {:02}/16 | {} | {} | [{}]",
            self.current_step + 1,
            bpm_str,
            active_freq_str,
            msg
        );
        draw_string(fb, client.x + 8, status_y + 2, &status_text, Color::TEXT_DIM, None);
    }
}
