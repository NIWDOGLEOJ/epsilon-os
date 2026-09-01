//! Retro Snake Arcade Game for AegisOS
//!
//! Features 20x20 grid game loop, smooth Arrow / WASD control, food spawning,
//! collision detection, real-time score tracking, and restart on Spacebar.

use alloc::collections::VecDeque;
use alloc::format;
use crate::drivers::framebuffer::Framebuffer;
use crate::drivers::ps2_keyboard::{KeyCode, KeyEvent};
use crate::gui::font::draw_string;
use crate::gui::primitives::{draw_rect, draw_rect_outline, draw_rounded_rect, Color, Rect};
use crate::gui::window::Window;

const GRID_SIZE: i32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

pub struct SnakeApp {
    pub snake: VecDeque<Point>,
    pub dir: Direction,
    pub next_dir: Direction,
    pub food: Point,
    pub score: u32,
    pub high_score: u32,
    pub game_over: bool,
    pub paused: bool,
    pub tick_counter: u32,
    pub rng_state: u32,
}

impl SnakeApp {
    pub fn new() -> Self {
        let mut snake = VecDeque::new();
        snake.push_back(Point { x: 10, y: 10 });
        snake.push_back(Point { x: 9, y: 10 });
        snake.push_back(Point { x: 8, y: 10 });

        let mut app = Self {
            snake,
            dir: Direction::Right,
            next_dir: Direction::Right,
            food: Point { x: 15, y: 10 },
            score: 0,
            high_score: 0,
            game_over: false,
            paused: false,
            tick_counter: 0,
            rng_state: 0x12345678,
        };
        app.spawn_food();
        app
    }

    fn next_random(&mut self) -> u32 {
        self.rng_state = self.rng_state.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng_state
    }

    pub fn spawn_food(&mut self) {
        let mut attempts = 0;
        loop {
            attempts += 1;
            let fx = (self.next_random() % GRID_SIZE as u32) as i32;
            let fy = (self.next_random() % GRID_SIZE as u32) as i32;
            let p = Point { x: fx, y: fy };
            if !self.snake.contains(&p) || attempts > 100 {
                self.food = p;
                break;
            }
        }
    }

    pub fn restart(&mut self) {
        self.snake.clear();
        self.snake.push_back(Point { x: 10, y: 10 });
        self.snake.push_back(Point { x: 9, y: 10 });
        self.snake.push_back(Point { x: 8, y: 10 });
        self.dir = Direction::Right;
        self.next_dir = Direction::Right;
        if self.score > self.high_score {
            self.high_score = self.score;
        }
        self.score = 0;
        self.game_over = false;
        self.paused = false;
        self.spawn_food();
    }

    pub fn update(&mut self) {
        if self.game_over || self.paused {
            return;
        }

        self.tick_counter += 1;
        if self.tick_counter < 6 {
            return; // Game speed throttle
        }
        self.tick_counter = 0;

        self.dir = self.next_dir;
        let head = *self.snake.front().unwrap();

        let new_head = match self.dir {
            Direction::Up => Point { x: head.x, y: head.y - 1 },
            Direction::Down => Point { x: head.x, y: head.y + 1 },
            Direction::Left => Point { x: head.x - 1, y: head.y },
            Direction::Right => Point { x: head.x + 1, y: head.y },
        };

        // Wall collision
        if new_head.x < 0 || new_head.x >= GRID_SIZE || new_head.y < 0 || new_head.y >= GRID_SIZE {
            self.game_over = true;
            if self.score > self.high_score {
                self.high_score = self.score;
            }
            return;
        }

        // Self collision
        if self.snake.contains(&new_head) {
            self.game_over = true;
            if self.score > self.high_score {
                self.high_score = self.score;
            }
            return;
        }

        self.snake.push_front(new_head);

        // Check food
        if new_head == self.food {
            self.score += 10;
            self.spawn_food();
        } else {
            self.snake.pop_back();
        }
    }

    pub fn handle_key(&mut self, event: KeyEvent) {
        if !event.pressed {
            return;
        }

        match event.code {
            KeyCode::Up => {
                if self.dir != Direction::Down {
                    self.next_dir = Direction::Up;
                }
            }
            KeyCode::Down => {
                if self.dir != Direction::Up {
                    self.next_dir = Direction::Down;
                }
            }
            KeyCode::Left => {
                if self.dir != Direction::Right {
                    self.next_dir = Direction::Left;
                }
            }
            KeyCode::Right => {
                if self.dir != Direction::Left {
                    self.next_dir = Direction::Right;
                }
            }
            KeyCode::Printable(c) => match c as char {
                'w' | 'W' => {
                    if self.dir != Direction::Down {
                        self.next_dir = Direction::Up;
                    }
                }
                's' | 'S' => {
                    if self.dir != Direction::Up {
                        self.next_dir = Direction::Down;
                    }
                }
                'a' | 'A' => {
                    if self.dir != Direction::Right {
                        self.next_dir = Direction::Left;
                    }
                }
                'd' | 'D' => {
                    if self.dir != Direction::Left {
                        self.next_dir = Direction::Right;
                    }
                }
                ' ' => {
                    if self.game_over {
                        self.restart();
                    } else {
                        self.paused = !self.paused;
                    }
                }
                'r' | 'R' => self.restart(),
                _ => {}
            },
            KeyCode::Enter => {
                if self.game_over {
                    self.restart();
                }
            }
            _ => {}
        }
    }

    pub fn render(&mut self, win: &Window, fb: &mut Framebuffer) {
        self.update();

        let client = win.client_rect();
        if client.width < 220 || client.height < 240 {
            return;
        }

        // Dark background
        draw_rect(fb, client, Color::rgb(20, 24, 28));

        // Header info bar
        let score_str = format!("Score: {}  High: {}", self.score, self.high_score);
        draw_string(fb, client.x + 10, client.y + 6, &score_str, Color::TEXT_HIGHLIGHT, None);

        let status_str = if self.game_over {
            "GAME OVER! Press Space/R"
        } else if self.paused {
            "[PAUSED]"
        } else {
            "Arrows/WASD to Move"
        };
        draw_string(fb, client.right() - 180, client.y + 6, status_str, Color::rgb(255, 200, 80), None);

        // Play grid arena
        let arena_x = client.x + 10;
        let arena_y = client.y + 28;
        let arena_size = (client.height.saturating_sub(38)).min(client.width.saturating_sub(20));
        let cell_size = (arena_size / GRID_SIZE as u32).max(4);
        let grid_px = cell_size * GRID_SIZE as u32;

        let arena_rect = Rect::new(arena_x, arena_y, grid_px, grid_px);
        draw_rect(fb, arena_rect, Color::rgb(10, 12, 16));
        draw_rect_outline(fb, arena_rect, Color::rgb(50, 56, 68), 1);

        // Render Food (Red square with round corners)
        let fx = arena_x + (self.food.x as u32 * cell_size) as i32;
        let fy = arena_y + (self.food.y as u32 * cell_size) as i32;
        draw_rounded_rect(
            fb,
            Rect::new(fx + 1, fy + 1, cell_size - 2, cell_size - 2),
            2,
            Color::rgb(255, 80, 80),
        );

        // Render Snake Body & Head
        for (i, segment) in self.snake.iter().enumerate() {
            let sx = arena_x + (segment.x as u32 * cell_size) as i32;
            let sy = arena_y + (segment.y as u32 * cell_size) as i32;
            let color = if i == 0 {
                Color::rgb(80, 250, 123) // Green head
            } else {
                Color::rgb(40, 180, 90)  // Darker body
            };
            draw_rounded_rect(
                fb,
                Rect::new(sx + 1, sy + 1, cell_size - 2, cell_size - 2),
                2,
                color,
            );
        }
    }
}
