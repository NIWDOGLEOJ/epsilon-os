//! Minesweeper Retro Arcade Game for AegisOS
//!
//! Features 9x9 Beginner (10 mines) and 16x16 Intermediate (40 mines) modes,
//! first-click safety guarantee, recursive zero-neighbor flood reveal,
//! right-click/Shift-click flag placement, 3-digit red digital LED counters,
//! and an animated yellow smiley face button (🙂, 😮, 😎, 😵).

use alloc::vec;
use alloc::vec::Vec;

use crate::drivers::framebuffer::Framebuffer;
use crate::drivers::ps2_keyboard::{KeyCode, KeyEvent};
use crate::gui::font::{draw_char, FONT_HEIGHT, FONT_WIDTH};
use crate::gui::primitives::{
    draw_circle, draw_circle_outline, draw_line, draw_rect, Color, Rect,
};
use crate::gui::window::Window;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Beginner,     // 9x9, 10 mines
    Intermediate, // 16x16, 40 mines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    Ready,   // Waiting for first click
    Playing, // Timer running
    Won,     // All safe cells revealed
    Lost,    // Stepped on a mine
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmileyState {
    Normal,    // 🙂
    Surprised, // 😮 (when mouse pressed on board)
    Won,       // 😎
    Dead,      // 😵
}

#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub has_mine: bool,
    pub revealed: bool,
    pub flagged: bool,
    pub neighbor_mines: u8,
    pub exploded: bool,
}

impl Cell {
    pub const fn empty() -> Self {
        Self {
            has_mine: false,
            revealed: false,
            flagged: false,
            neighbor_mines: 0,
            exploded: false,
        }
    }
}

pub struct MinesweeperApp {
    pub difficulty: Difficulty,
    pub cols: usize,
    pub rows: usize,
    pub total_mines: usize,
    pub grid: Vec<Cell>,
    pub game_state: GameState,
    pub flags_count: usize,
    pub start_tick: u64,
    pub elapsed_seconds: u32,
    pub smiley_state: SmileyState,
    pub mouse_is_down: bool,
}

impl MinesweeperApp {
    pub fn new() -> Self {
        let mut app = Self {
            difficulty: Difficulty::Beginner,
            cols: 9,
            rows: 9,
            total_mines: 10,
            grid: Vec::new(),
            game_state: GameState::Ready,
            flags_count: 0,
            start_tick: 0,
            elapsed_seconds: 0,
            smiley_state: SmileyState::Normal,
            mouse_is_down: false,
        };
        app.reset_board();
        app
    }

    /// Resets the game board to the current difficulty.
    pub fn reset_board(&mut self) {
        match self.difficulty {
            Difficulty::Beginner => {
                self.cols = 9;
                self.rows = 9;
                self.total_mines = 10;
            }
            Difficulty::Intermediate => {
                self.cols = 16;
                self.rows = 16;
                self.total_mines = 40;
            }
        }
        self.grid = vec![Cell::empty(); self.cols * self.rows];
        self.game_state = GameState::Ready;
        self.flags_count = 0;
        self.start_tick = 0;
        self.elapsed_seconds = 0;
        self.smiley_state = SmileyState::Normal;
        self.mouse_is_down = false;
    }

    pub fn set_difficulty(&mut self, diff: Difficulty) {
        if self.difficulty != diff {
            self.difficulty = diff;
            self.reset_board();
        }
    }

    #[inline]
    fn idx(&self, col: usize, row: usize) -> usize {
        row * self.cols + col
    }

    /// Populates mines randomly, guaranteeing the clicked (safe_c, safe_r)
    /// and its adjacent 8 neighbors will NOT have mines.
    pub fn generate_mines(&mut self, safe_c: usize, safe_r: usize) {
        let _total_cells = self.cols * self.rows;
        let mut seed = crate::task::get_uptime_ticks()
            .wrapping_add(safe_c as u64 * 31)
            .wrapping_add(safe_r as u64 * 127);
        if seed == 0 {
            seed = 0xACE1;
        }

        let mut placed = 0;
        while placed < self.total_mines {
            // Xorshift64 PRNG
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;

            let c = (seed as usize) % self.cols;
            seed ^= seed >> 9;
            let r = (seed as usize) % self.rows;

            // Check if cell is in the 3x3 exclusion zone around first click
            let dc = (c as isize - safe_c as isize).abs();
            let dr = (r as isize - safe_r as isize).abs();
            if dc <= 1 && dr <= 1 {
                continue;
            }

            let i = self.idx(c, r);
            if !self.grid[i].has_mine {
                self.grid[i].has_mine = true;
                placed += 1;
            }
        }

        // Calculate neighbor counts
        for r in 0..self.rows {
            for c in 0..self.cols {
                let i = self.idx(c, r);
                if self.grid[i].has_mine {
                    continue;
                }
                let mut count = 0u8;
                for dr in -1isize..=1 {
                    for dc in -1isize..=1 {
                        if dc == 0 && dr == 0 {
                            continue;
                        }
                        let nc = c as isize + dc;
                        let nr = r as isize + dr;
                        if nc >= 0 && nc < self.cols as isize && nr >= 0 && nr < self.rows as isize {
                            if self.grid[self.idx(nc as usize, nr as usize)].has_mine {
                                count += 1;
                            }
                        }
                    }
                }
                self.grid[i].neighbor_mines = count;
            }
        }
    }

    /// Handles a left-click on cell (c, r).
    pub fn reveal_cell(&mut self, col: usize, row: usize) {
        if col >= self.cols || row >= self.rows {
            return;
        }
        if self.game_state == GameState::Won || self.game_state == GameState::Lost {
            return;
        }

        let i = self.idx(col, row);
        if self.grid[i].flagged || self.grid[i].revealed {
            return;
        }

        // First-click initialization
        if self.game_state == GameState::Ready {
            self.generate_mines(col, row);
            self.game_state = GameState::Playing;
            self.start_tick = crate::task::get_uptime_ticks();
        }

        // Clicked a mine!
        if self.grid[i].has_mine {
            self.grid[i].revealed = true;
            self.grid[i].exploded = true;
            self.game_state = GameState::Lost;
            self.smiley_state = SmileyState::Dead;
            crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::Alert);

            // Reveal all other mines
            for cell in self.grid.iter_mut() {
                if cell.has_mine {
                    cell.revealed = true;
                }
            }
            return;
        }

        // Reveal safe cell
        self.grid[i].revealed = true;
        crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::SnakeEat);

        // Recursive flood-fill if 0 neighbor mines
        if self.grid[i].neighbor_mines == 0 {
            self.flood_reveal(col, row);
        }

        // Check victory
        self.check_victory();
    }

    /// Recursive zero-neighbor flood reveal using an iterative queue (heap-safe in no_std).
    fn flood_reveal(&mut self, start_c: usize, start_r: usize) {
        let mut queue: Vec<(usize, usize)> = Vec::new();
        queue.push((start_c, start_r));

        while let Some((c, r)) = queue.pop() {
            for dr in -1isize..=1 {
                for dc in -1isize..=1 {
                    if dc == 0 && dr == 0 {
                        continue;
                    }
                    let nc = c as isize + dc;
                    let nr = r as isize + dr;
                    if nc >= 0 && nc < self.cols as isize && nr >= 0 && nr < self.rows as isize {
                        let ni = self.idx(nc as usize, nr as usize);
                        let cell = &mut self.grid[ni];
                        if !cell.revealed && !cell.flagged && !cell.has_mine {
                            cell.revealed = true;
                            if cell.neighbor_mines == 0 {
                                queue.push((nc as usize, nr as usize));
                            }
                        }
                    }
                }
            }
        }
    }

    /// Toggles a flag marker on cell (c, r).
    pub fn toggle_flag(&mut self, col: usize, row: usize) {
        if col >= self.cols || row >= self.rows {
            return;
        }
        if self.game_state != GameState::Playing && self.game_state != GameState::Ready {
            return;
        }

        let i = self.idx(col, row);
        if self.grid[i].revealed {
            return;
        }

        if self.grid[i].flagged {
            self.grid[i].flagged = false;
            if self.flags_count > 0 {
                self.flags_count -= 1;
            }
        } else {
            if self.flags_count < self.total_mines {
                self.grid[i].flagged = true;
                self.flags_count += 1;
            }
        }
        crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::SnakeEat);
    }

    /// Checks if all safe cells have been revealed.
    pub fn check_victory(&mut self) {
        let total_cells = self.cols * self.rows;
        let revealed_count = self.grid.iter().filter(|c| c.revealed).count();

        if revealed_count == total_cells - self.total_mines {
            self.game_state = GameState::Won;
            self.smiley_state = SmileyState::Won;
            // Flag remaining mines automatically
            for cell in self.grid.iter_mut() {
                if cell.has_mine {
                    cell.flagged = true;
                }
            }
            self.flags_count = self.total_mines;
            crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::WindowSnap);
        }
    }

    /// Updates elapsed timer while playing.
    pub fn update_timer(&mut self) {
        if self.game_state == GameState::Playing {
            let current = crate::task::get_uptime_ticks();
            let diff = current.saturating_sub(self.start_tick);
            self.elapsed_seconds = (diff / 100).min(999) as u32;
        }
    }

    /// Dispatches mouse clicks inside the Minesweeper window.
    pub fn handle_mouse_down(&mut self, win: &Window, x: i32, y: i32, is_right: bool, shift: bool) {
        let client = win.client_rect();
        self.mouse_is_down = true;

        // Smiley face button: centered in dashboard at y = client.y + 8, size 26x26
        let smiley_x = client.x + (client.width as i32 - 26) / 2;
        let smiley_y = client.y + 8;
        let smiley_rect = Rect::new(smiley_x, smiley_y, 26, 26);
        if smiley_rect.contains(x, y) {
            self.reset_board();
            return;
        }

        // Difficulty selector buttons: [9x9] at x = client.x + 8, [16x16] at x = client.x + 52
        let diff9_rect = Rect::new(client.x + 8, client.y + 11, 38, 20);
        if diff9_rect.contains(x, y) {
            self.set_difficulty(Difficulty::Beginner);
            return;
        }
        let diff16_rect = Rect::new(client.x + 50, client.y + 11, 46, 20);
        if diff16_rect.contains(x, y) {
            self.set_difficulty(Difficulty::Intermediate);
            return;
        }

        // Board area
        let board_y = client.y + 42;
        let cell_size = 24i32;
        let board_x = client.x + (client.width as i32 - (self.cols as i32 * cell_size)) / 2;

        if x >= board_x && y >= board_y {
            let col = (x - board_x) / cell_size;
            let row = (y - board_y) / cell_size;
            if col >= 0 && col < self.cols as i32 && row >= 0 && row < self.rows as i32 {
                let (c, r) = (col as usize, row as usize);
                if is_right || shift {
                    self.toggle_flag(c, r);
                } else {
                    if self.game_state != GameState::Lost && self.game_state != GameState::Won {
                        self.smiley_state = SmileyState::Surprised;
                    }
                    self.reveal_cell(c, r);
                }
            }
        }
    }

    pub fn handle_mouse_up(&mut self) {
        self.mouse_is_down = false;
        if self.game_state == GameState::Playing || self.game_state == GameState::Ready {
            self.smiley_state = SmileyState::Normal;
        }
    }

    pub fn handle_key(&mut self, event: KeyEvent) {
        if !event.pressed {
            return;
        }
        match event.code {
            KeyCode::Printable(b'r') | KeyCode::Printable(b'R') => self.reset_board(),
            KeyCode::Printable(b'1') => self.set_difficulty(Difficulty::Beginner),
            KeyCode::Printable(b'2') => self.set_difficulty(Difficulty::Intermediate),
            _ => {}
        }
    }

    /// Renders the entire Minesweeper UI.
    pub fn render(&mut self, win: &Window, fb: &mut Framebuffer) {
        self.update_timer();

        let client = win.client_rect();
        if client.width < 100 || client.height < 100 {
            return;
        }

        // Classic retro light gray background
        let bg_color = Color::rgb(192, 192, 192);
        draw_rect(fb, client, bg_color);

        // Outer beveled border
        self.draw_3d_inset(fb, client.x + 4, client.y + 4, client.width - 8, client.height - 8);

        // ── Top Retro Dashboard ──
        let dash_h = 32u32;
        let dash_y = client.y + 6;
        let dash_w = client.width - 16;
        let dash_x = client.x + 8;
        self.draw_3d_sunken(fb, dash_x, dash_y, dash_w, dash_h);

        // Remaining Mines LED Display (e.g. 010)
        let remaining_mines = (self.total_mines as isize - self.flags_count as isize).max(0) as u32;
        self.draw_led_display(fb, dash_x + 6, dash_y + 4, remaining_mines);

        // Smiley Face Reset Button (Center)
        let smiley_x = client.x + (client.width as i32 - 24) / 2;
        let smiley_y = dash_y + 4;
        self.draw_smiley_button(fb, smiley_x, smiley_y);

        // Elapsed Timer LED Display (Right)
        let timer_x = dash_x + dash_w as i32 - 46;
        self.draw_led_display(fb, timer_x, dash_y + 4, self.elapsed_seconds);

        // ── Grid Board Area ──
        let cell_size = 24i32;
        let board_w = (self.cols as i32 * cell_size) as u32;
        let board_h = (self.rows as i32 * cell_size) as u32;
        let board_x = client.x + (client.width as i32 - board_w as i32) / 2;
        let board_y = client.y + 44;

        // Inset border around grid
        self.draw_3d_sunken(fb, board_x - 3, board_y - 3, board_w + 6, board_h + 6);

        // Draw cells
        for r in 0..self.rows {
            for c in 0..self.cols {
                let cx = board_x + (c as i32 * cell_size);
                let cy = board_y + (r as i32 * cell_size);
                let cell = &self.grid[self.idx(c, r)];
                self.draw_cell(fb, cx, cy, cell_size as u32, cell);
            }
        }
    }

    /// Draws a single grid cell (beveled covered, revealed number, mine, or flag).
    fn draw_cell(&self, fb: &mut Framebuffer, x: i32, y: i32, size: u32, cell: &Cell) {
        let light = Color::WHITE;
        let dark = Color::rgb(128, 128, 128);
        let base_gray = Color::rgb(192, 192, 192);

        if !cell.revealed {
            // Covered cell: 3D raised button
            draw_rect(fb, Rect::new(x, y, size, size), base_gray);
            // Top and left light border (2px)
            draw_line(fb, x, y, x + size as i32 - 1, y, light);
            draw_line(fb, x, y + 1, x + size as i32 - 2, y + 1, light);
            draw_line(fb, x, y, x, y + size as i32 - 1, light);
            draw_line(fb, x + 1, y, x + 1, y + size as i32 - 2, light);

            // Bottom and right dark border (2px)
            draw_line(fb, x, y + size as i32 - 1, x + size as i32 - 1, y + size as i32 - 1, dark);
            draw_line(fb, x + 1, y + size as i32 - 2, x + size as i32 - 1, y + size as i32 - 2, dark);
            draw_line(fb, x + size as i32 - 1, y, x + size as i32 - 1, y + size as i32 - 1, dark);
            draw_line(fb, x + size as i32 - 2, y + 1, x + size as i32 - 2, y + size as i32 - 1, dark);

            if cell.flagged {
                self.draw_flag(fb, x, y);
            }
        } else {
            // Revealed cell: flat with thin 1px border
            let fill_color = if cell.exploded {
                Color::rgb(255, 60, 60) // Red background on detonated mine
            } else {
                base_gray
            };
            draw_rect(fb, Rect::new(x, y, size, size), fill_color);
            draw_line(fb, x, y, x + size as i32 - 1, y, dark);
            draw_line(fb, x, y, x, y + size as i32 - 1, dark);

            if cell.has_mine {
                self.draw_mine(fb, x, y);
            } else if cell.neighbor_mines > 0 {
                let num_color = match cell.neighbor_mines {
                    1 => Color::rgb(0, 0, 255),       // 1 = Blue
                    2 => Color::rgb(0, 128, 0),       // 2 = Green
                    3 => Color::rgb(255, 0, 0),       // 3 = Red
                    4 => Color::rgb(0, 0, 128),       // 4 = Dark Blue
                    5 => Color::rgb(128, 0, 0),       // 5 = Dark Red
                    6 => Color::rgb(0, 128, 128),     // 6 = Cyan / Teal
                    7 => Color::rgb(0, 0, 0),         // 7 = Black
                    8 => Color::rgb(128, 128, 128),   // 8 = Gray
                    _ => Color::BLACK,
                };
                let ch = b'0' + cell.neighbor_mines;
                let tx = x + (size as i32 - FONT_WIDTH as i32) / 2;
                let ty = y + (size as i32 - FONT_HEIGHT as i32) / 2;
                draw_char(fb, tx, ty, ch, num_color, None);
            }
        }
    }

    /// Draws a red flag marker on a covered cell.
    fn draw_flag(&self, fb: &mut Framebuffer, x: i32, y: i32) {
        let flag_red = Color::rgb(220, 20, 20);
        let pole_black = Color::BLACK;

        // Vertical pole
        draw_line(fb, x + 13, y + 6, x + 13, y + 17, pole_black);

        // Flag base
        draw_line(fb, x + 9, y + 18, x + 17, y + 18, pole_black);
        draw_line(fb, x + 11, y + 17, x + 15, y + 17, pole_black);

        // Red triangular flag
        for row in 0..6 {
            let fw = 6 - row;
            draw_line(fb, x + 13 - fw as i32, y + 6 + row as i32, x + 13, y + 6 + row as i32, flag_red);
        }
    }

    /// Draws a black contact naval mine with spikes and white highlight.
    fn draw_mine(&self, fb: &mut Framebuffer, x: i32, y: i32) {
        let black = Color::BLACK;
        let white = Color::WHITE;
        let cx = x + 12;
        let cy = y + 12;

        // Main body
        draw_circle(fb, cx, cy, 5, black);

        // Spikes (cross & diagonals)
        draw_line(fb, cx - 8, cy, cx + 8, cy, black);
        draw_line(fb, cx, cy - 8, cx, cy + 8, black);
        draw_line(fb, cx - 6, cy - 6, cx + 6, cy + 6, black);
        draw_line(fb, cx - 6, cy + 6, cx + 6, cy - 6, black);

        // White glint / reflection dot
        draw_circle(fb, cx - 2, cy - 2, 1, white);
    }

    /// Draws 3-digit digital 7-segment LED counter (red on black).
    fn draw_led_display(&self, fb: &mut Framebuffer, x: i32, y: i32, val: u32) {
        let val_clamped = val.min(999);
        let d1 = (val_clamped / 100) % 10;
        let d2 = (val_clamped / 10) % 10;
        let d3 = val_clamped % 10;

        // Dark background inset
        draw_rect(fb, Rect::new(x, y, 40, 24), Color::BLACK);
        self.draw_3d_sunken(fb, x - 1, y - 1, 42, 26);

        let red = Color::rgb(255, 30, 30);
        let digits = [d1, d2, d3];
        for (i, &d) in digits.iter().enumerate() {
            let dx = x + 4 + (i as i32 * 12);
            let ch = b'0' + d as u8;
            draw_char(fb, dx, y + 4, ch, red, None);
        }
    }

    /// Draws the interactive yellow smiley face reset button.
    fn draw_smiley_button(&self, fb: &mut Framebuffer, x: i32, y: i32) {
        let size = 24u32;
        let base_gray = Color::rgb(192, 192, 192);

        // 3D raised button frame
        draw_rect(fb, Rect::new(x, y, size, size), base_gray);
        draw_line(fb, x, y, x + size as i32 - 1, y, Color::WHITE);
        draw_line(fb, x, y, x, y + size as i32 - 1, Color::WHITE);
        draw_line(fb, x, y + size as i32 - 1, x + size as i32 - 1, y + size as i32 - 1, Color::rgb(128, 128, 128));
        draw_line(fb, x + size as i32 - 1, y, x + size as i32 - 1, y + size as i32 - 1, Color::rgb(128, 128, 128));

        // Yellow face
        let cx = x + 12;
        let cy = y + 12;
        let yellow = Color::rgb(255, 215, 0);
        let black = Color::BLACK;
        draw_circle(fb, cx, cy, 8, yellow);
        draw_circle_outline(fb, cx, cy, 8, Color::rgb(160, 130, 0));

        match self.smiley_state {
            SmileyState::Normal => {
                // 🙂 Two eyes
                draw_circle(fb, cx - 3, cy - 2, 1, black);
                draw_circle(fb, cx + 3, cy - 2, 1, black);
                // Smile curve
                draw_line(fb, cx - 4, cy + 3, cx - 2, cy + 5, black);
                draw_line(fb, cx - 2, cy + 5, cx + 2, cy + 5, black);
                draw_line(fb, cx + 2, cy + 5, cx + 4, cy + 3, black);
            }
            SmileyState::Surprised => {
                // 😮 Wide eyes & round open mouth
                draw_circle(fb, cx - 3, cy - 2, 1, black);
                draw_circle(fb, cx + 3, cy - 2, 1, black);
                draw_circle_outline(fb, cx, cy + 3, 2, black);
            }
            SmileyState::Won => {
                // 😎 Cool sunglasses
                draw_line(fb, cx - 6, cy - 2, cx + 6, cy - 2, black);
                draw_rect(fb, Rect::new(cx - 5, cy - 2, 4, 3), black);
                draw_rect(fb, Rect::new(cx + 1, cy - 2, 4, 3), black);
                // Cool grin
                draw_line(fb, cx - 3, cy + 4, cx + 3, cy + 4, black);
            }
            SmileyState::Dead => {
                // 😵 X eyes
                // Left X
                draw_line(fb, cx - 5, cy - 4, cx - 2, cy - 1, black);
                draw_line(fb, cx - 2, cy - 4, cx - 5, cy - 1, black);
                // Right X
                draw_line(fb, cx + 2, cy - 4, cx + 5, cy - 1, black);
                draw_line(fb, cx + 5, cy - 4, cx + 2, cy - 1, black);
                // Frown curve
                draw_line(fb, cx - 3, cy + 5, cx, cy + 3, black);
                draw_line(fb, cx, cy + 3, cx + 3, cy + 5, black);
            }
        }
    }

    /// Classic Windows-style 3D inset border.
    fn draw_3d_inset(&self, fb: &mut Framebuffer, x: i32, y: i32, w: u32, h: u32) {
        let dark = Color::rgb(128, 128, 128);
        let light = Color::WHITE;
        let (x2, y2) = (x + w as i32 - 1, y + h as i32 - 1);
        draw_line(fb, x, y, x2, y, dark);
        draw_line(fb, x, y, x, y2, dark);
        draw_line(fb, x, y2, x2, y2, light);
        draw_line(fb, x2, y, x2, y2, light);
    }

    /// Classic Windows-style 3D sunken well.
    fn draw_3d_sunken(&self, fb: &mut Framebuffer, x: i32, y: i32, w: u32, h: u32) {
        let dark = Color::rgb(128, 128, 128);
        let light = Color::WHITE;
        let (x2, y2) = (x + w as i32 - 1, y + h as i32 - 1);
        draw_line(fb, x, y, x2, y, dark);
        draw_line(fb, x, y, x, y2, dark);
        draw_line(fb, x + 1, y2, x2, y2, light);
        draw_line(fb, x2, y + 1, x2, y2, light);
    }
}
