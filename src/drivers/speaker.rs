//! Hardware PC Speaker Driver & Music Sequencer for AegisOS
//!
//! Controls the x86 8253/8254 Programmable Interval Timer (PIT) Channel 2
//! (Ports 0x42, 0x43) and System Control Port B (0x61) to generate square-wave
//! audio. Features a non-blocking compositor-driven audio sequencer and
//! predefined system sound effects.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::serial::{inb, outb};
use crate::arch::InterruptGuard;

pub const PIT_CHANNEL2_DATA: u16 = 0x42;
pub const PIT_COMMAND_PORT: u16 = 0x43;
pub const SPEAKER_CONTROL_PORT: u16 = 0x61;
pub const PIT_BASE_FREQ: u32 = 1_193_182;

/// Musical note representation with frequency and frame duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub freq_hz: u32,
    pub duration_frames: u32,
}

impl Note {
    pub const fn new(freq_hz: u32, duration_frames: u32) -> Self {
        Self { freq_hz, duration_frames }
    }

    pub const fn rest(duration_frames: u32) -> Self {
        Self { freq_hz: 0, duration_frames }
    }
}

/// Predefined system sound effects for OS feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundEffect {
    BootChime,
    WindowOpen,
    WindowClose,
    WindowSnap,
    Alert,
    SnakeEat,
    SnakeDie,
    BeepSuccess,
}

/// Low-level: sets the hardware speaker frequency via PIT Channel 2 and Port 0x61.
pub fn set_frequency(freq_hz: u32) {
    let _guard = InterruptGuard::acquire();
    if freq_hz == 0 {
        mute();
        return;
    }

    let divisor = (PIT_BASE_FREQ / freq_hz).clamp(1, 65535) as u16;

    unsafe {
        // Mode 3: Square wave generator, Channel 2, LSB then MSB, 16-bit binary
        outb(PIT_COMMAND_PORT, 0xB6);
        outb(PIT_CHANNEL2_DATA, (divisor & 0xFF) as u8);
        outb(PIT_CHANNEL2_DATA, ((divisor >> 8) & 0xFF) as u8);

        // Turn on speaker: bit 0 (timer 2 gate) and bit 1 (speaker data enable)
        let val = inb(SPEAKER_CONTROL_PORT);
        if (val & 0x03) != 0x03 {
            outb(SPEAKER_CONTROL_PORT, val | 0x03);
        }
    }
}

/// Low-level: immediately silences the speaker by clearing Port 0x61 bits 0 & 1.
pub fn mute() {
    let _guard = InterruptGuard::acquire();
    unsafe {
        let val = inb(SPEAKER_CONTROL_PORT);
        outb(SPEAKER_CONTROL_PORT, val & 0xFC);
    }
}

/// Low-level: returns true if Port 0x61 bits 0 & 1 are actively asserted.
pub fn is_speaker_active() -> bool {
    let _guard = InterruptGuard::acquire();
    unsafe {
        let val = inb(SPEAKER_CONTROL_PORT);
        (val & 0x03) == 0x03
    }
}

/// Returns the current raw byte of System Control Port B (0x61).
pub fn read_speaker_port() -> u8 {
    let _guard = InterruptGuard::acquire();
    unsafe { inb(SPEAKER_CONTROL_PORT) }
}

/// Non-blocking audio sequencer state.
pub struct AudioPlayer {
    pub current_frames_remaining: u32,
    pub active_freq: u32,
    pub queue: VecDeque<Note>,
    pub enabled: bool,
}

impl AudioPlayer {
    pub const fn new() -> Self {
        Self {
            current_frames_remaining: 0,
            active_freq: 0,
            queue: VecDeque::new(),
            enabled: true,
        }
    }

    /// Clears any pending notes and mutes the hardware speaker.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.current_frames_remaining = 0;
        self.active_freq = 0;
        mute();
    }

    /// Enqueues a slice of notes to play sequentially.
    pub fn enqueue_notes(&mut self, notes: &[Note]) {
        if !self.enabled {
            return;
        }
        for &note in notes {
            self.queue.push_back(note);
        }
    }

    /// Advances the sequencer by 1 frame (called at 60 FPS from desktop compositor loop).
    pub fn step(&mut self) {
        if !self.enabled {
            if self.active_freq != 0 {
                self.clear();
            }
            return;
        }

        if self.current_frames_remaining > 0 {
            self.current_frames_remaining -= 1;
            if self.current_frames_remaining > 0 {
                return;
            }
        }

        // Note expired or idle: fetch next note from queue
        if let Some(next_note) = self.queue.pop_front() {
            self.active_freq = next_note.freq_hz;
            self.current_frames_remaining = next_note.duration_frames.max(1);
            set_frequency(next_note.freq_hz);
        } else if self.active_freq != 0 {
            self.active_freq = 0;
            mute();
        }
    }
}

pub static AUDIO_PLAYER: Mutex<AudioPlayer> = Mutex::new(AudioPlayer::new());

/// Initializes the hardware PC speaker driver, ensuring it starts muted.
pub fn init_speaker() {
    mute();
    crate::serial_println!("[OK] Hardware PC Speaker Driver (PIT Channel 2 & Port 0x61) initialized.");
}

/// Steps the audio player frame counter (call once per 60 FPS compositor frame).
pub fn update_audio() {
    let _guard = InterruptGuard::acquire();
    let mut audio_player = AUDIO_PLAYER.lock();
    audio_player.step();
}

/// Enqueues a single tone for playback.
pub fn beep(freq_hz: u32, duration_ms: u32) {
    // 60 FPS frame calculation: duration_ms * 60 / 1000 with integer rounding
    let frames = ((duration_ms as u64 * 60 + 500) / 1000).max(1) as u32;
    let _guard = InterruptGuard::acquire();
    let mut audio_player = AUDIO_PLAYER.lock();
    audio_player.enqueue_notes(&[Note::new(freq_hz, frames)]);
}

/// Enqueues a slice of notes for playback.
pub fn play_notes(notes: &[Note]) {
    let _guard = InterruptGuard::acquire();
    let mut audio_player = AUDIO_PLAYER.lock();
    audio_player.enqueue_notes(notes);
}

/// Dispatches a predefined system sound effect.
pub fn play_sound_effect(sfx: SoundEffect) {
    let notes: Vec<Note> = match sfx {
        // Major arpeggio chord: C5 -> E5 -> G5 -> C6
        SoundEffect::BootChime => alloc::vec![
            Note::new(523, 4),
            Note::new(659, 4),
            Note::new(784, 4),
            Note::new(1046, 8),
        ],
        // Ascending chirp
        SoundEffect::WindowOpen => alloc::vec![
            Note::new(880, 2),
            Note::new(1320, 3),
        ],
        // Descending chirp
        SoundEffect::WindowClose => alloc::vec![
            Note::new(1100, 2),
            Note::new(660, 3),
        ],
        // Window snap affirmative double-chirp
        SoundEffect::WindowSnap => alloc::vec![
            Note::new(740, 2),
            Note::new(1046, 3),
        ],
        // Double warning beep
        SoundEffect::Alert => alloc::vec![
            Note::new(440, 4),
            Note::rest(2),
            Note::new(440, 4),
        ],
        // Snake eating food: crisp blip
        SoundEffect::SnakeEat => alloc::vec![
            Note::new(988, 3),
        ],
        // Snake game over: descending crunch
        SoundEffect::SnakeDie => alloc::vec![
            Note::new(400, 3),
            Note::new(250, 3),
            Note::new(150, 5),
        ],
        // Success chime
        SoundEffect::BeepSuccess => alloc::vec![
            Note::new(587, 3),
            Note::new(880, 4),
        ],
    };

    let _guard = InterruptGuard::acquire();
    let mut audio_player = AUDIO_PLAYER.lock();
    audio_player.enqueue_notes(&notes);
}
