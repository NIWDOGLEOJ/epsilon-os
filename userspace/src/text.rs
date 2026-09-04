//! Colours and allocation-free formatting.
//!
//! There is no heap in Ring 3, so no `String` and no `format!`. These write
//! into caller-supplied byte slices instead, returning the new write position.

/// ARGB colours, matching the kernel's conventions.
pub const COLOR_BG: u32 = 0xFF10_1418;
pub const COLOR_FG: u32 = 0xFFD0_D8E0;
pub const COLOR_PROMPT: u32 = 0xFF4C_D964;
pub const COLOR_ERROR: u32 = 0xFFFF_5F56;
pub const COLOR_HEADING: u32 = 0xFF5A_C8FA;
pub const COLOR_CURSOR: u32 = 0xFFD0_D8E0;
pub const COLOR_WARN: u32 = 0xFFFF_BD2E;
pub const COLOR_DIM: u32 = 0xFF8A_94A0;

pub const COLOR_BUTTON: u32 = 0xFF2A_313A;
pub const COLOR_BUTTON_HOVER: u32 = 0xFF3D_4753;
pub const COLOR_BUTTON_DANGER: u32 = 0xFF5A_2A2A;
pub const COLOR_BUTTON_EDGE: u32 = 0xFF4A_545F;

/// Appends `value` in decimal at `pos`, returning the new position.
pub fn push_u64(buf: &mut [u8], mut pos: usize, value: u64) -> usize {
    let mut digits = [0u8; 20];
    let mut count = 0;
    let mut v = value;
    loop {
        digits[count] = b'0' + (v % 10) as u8;
        count += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    while count > 0 && pos < buf.len() {
        count -= 1;
        buf[pos] = digits[count];
        pos += 1;
    }
    pos
}

/// Appends bytes at `pos`, truncating at the end of `buf`.
pub fn push_str(buf: &mut [u8], mut pos: usize, s: &[u8]) -> usize {
    for &b in s {
        if pos >= buf.len() {
            break;
        }
        buf[pos] = b;
        pos += 1;
    }
    pos
}
