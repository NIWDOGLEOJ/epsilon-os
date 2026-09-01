//! AegisOS E2E Test Harness: Privilege, GDT, TSS, IDT and UART Serial Simulator
//!
//! Models hardware segment selectors, TSS stack switches, IDT vectors,
//! Ring 3 exception trapping, and COM1 serial telemetry.

use super::types::*;

pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_DS: u16 = 0x10;
pub const USER_DS: u16 = 0x18 | 3;
pub const USER_CS: u16 = 0x20 | 3;
pub const TSS_SEL: u16 = 0x28;

#[derive(Debug, Clone)]
pub struct TssSimulator {
    pub rsp0: u64,
    pub ist1: u64, // Double Fault stack
    pub ist2: u64, // Page Fault stack (if configured)
    pub ist3: u64,
    pub iomap_base: u16,
}

impl TssSimulator {
    pub fn new() -> Self {
        Self {
            rsp0: 0,
            ist1: 0,
            ist2: 0,
            ist3: 0,
            iomap_base: 104,
        }
    }

    pub fn set_rsp0(&mut self, stack_top: u64) {
        self.rsp0 = stack_top;
    }

    pub fn set_ist(&mut self, index: usize, stack_top: u64) {
        match index {
            1 => self.ist1 = stack_top,
            2 => self.ist2 = stack_top,
            3 => self.ist3 = stack_top,
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct IdtEntry {
    pub isr_offset: u64,
    pub segment_selector: u16,
    pub ist_index: u8,
    pub dpl: u8, // 0 = Kernel only, 3 = User callable
    pub present: bool,
}

impl IdtEntry {
    pub const fn empty() -> Self {
        Self {
            isr_offset: 0,
            segment_selector: 0,
            ist_index: 0,
            dpl: 0,
            present: false,
        }
    }
}

pub struct IdtSimulator {
    pub entries: [IdtEntry; 256],
}

impl IdtSimulator {
    pub fn new() -> Self {
        Self {
            entries: [IdtEntry::empty(); 256],
        }
    }

    pub fn set_handler(&mut self, vector: u8, isr_offset: u64, dpl: u8, ist: u8) {
        self.entries[vector as usize] = IdtEntry {
            isr_offset,
            segment_selector: KERNEL_CS,
            ist_index: ist,
            dpl,
            present: true,
        };
    }
}

pub struct UartSerialSimulator {
    buffer: Vec<String>,
    raw_bytes: Vec<u8>,
}

impl UartSerialSimulator {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            raw_bytes: Vec::new(),
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        self.raw_bytes.push(byte);
        if byte == b'\n' {
            if let Ok(line) = String::from_utf8(self.raw_bytes.clone()) {
                self.buffer.push(line.trim_end_matches(['\r', '\n']).to_string());
                self.raw_bytes.clear();
            }
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.write_byte(b);
        }
    }

    pub fn get_lines(&self) -> &[String] {
        &self.buffer
    }

    pub fn contains_log(&self, pattern: &str) -> bool {
        self.buffer.iter().any(|line| line.contains(pattern))
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.raw_bytes.clear();
    }
}
