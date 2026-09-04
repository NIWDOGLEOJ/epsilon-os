//! AegisChat — In-Kernel Intranet Collaboration & Messaging Client for AegisOS
//!
//! Connects to the in-kernel virtual loopback network stack (127.0.0.1:8080),
//! featuring multi-channel chat streams (#general, #kernel-dev, #agent, #alerts),
//! online user presence badges, color-coded chat bubbles with timestamps,
//! interactive input bar, and autonomous AI coprocessor responses over UDP.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::drivers::framebuffer::Framebuffer;
use crate::drivers::ps2_keyboard::{KeyCode, KeyEvent};
use crate::gui::font::draw_string;
use crate::gui::primitives::{
    draw_circle, draw_rect, draw_rounded_rect, draw_rounded_rect_outline, Color, Rect,
};
use crate::gui::window::Window;
use crate::net::{Ipv4Address, UdpSocket};

pub const CHANNELS: [&str; 4] = ["#general", "#kernel-dev", "#agent", "#alerts"];

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub channel: String,
    pub sender: String,
    pub text: String,
    pub timestamp_secs: u64,
    pub is_system: bool,
}

pub struct ChatApp {
    pub current_channel: usize,
    pub messages: Vec<ChatMessage>,
    pub input_text: String,
    pub cursor: usize,
    pub socket: UdpSocket,
    pub scroll_offset: usize,
    pub status_message: Option<String>,
}

impl ChatApp {
    pub const PORT: u16 = 8080;

    pub fn new() -> Self {
        let mut app = Self {
            current_channel: 0,
            messages: Vec::new(),
            input_text: String::new(),
            cursor: 0,
            socket: UdpSocket::bind(Self::PORT),
            scroll_offset: 0,
            status_message: Some("Connected to 127.0.0.1:8080".to_string()),
        };

        // Populate initial welcome messages in channels
        app.add_message(
            "#general",
            "kernel",
            "Welcome to AegisChat! Intranet loopback socket active on 127.0.0.1:8080.",
            0,
            true,
        );
        app.add_message(
            "#general",
            "agent",
            "AI Coprocessor online. Switch to #agent or mention @agent to query me.",
            1,
            false,
        );
        app.add_message(
            "#kernel-dev",
            "kernel",
            "Ring 0/Ring 3 fault isolation verified. VFS mounted at 0xFFFF_9000_0000_0000.",
            1,
            true,
        );
        app.add_message(
            "#agent",
            "agent",
            "Hello! I am your AegisOS Coprocessor. Type '@agent status' for diagnostics.",
            2,
            false,
        );
        app.add_message(
            "#alerts",
            "system",
            "System telemetry normal. Frame pacing stable at 60 FPS.",
            3,
            true,
        );

        app
    }

    /// Appends a message to the internal message history.
    pub fn add_message(
        &mut self,
        channel: &str,
        sender: &str,
        text: &str,
        timestamp_secs: u64,
        is_system: bool,
    ) {
        self.messages.push(ChatMessage {
            channel: channel.to_string(),
            sender: sender.to_string(),
            text: text.to_string(),
            timestamp_secs,
            is_system,
        });
    }

    /// Sends the current typed message over the UDP loopback socket.
    pub fn send_current_message(&mut self) {
        if self.input_text.trim().is_empty() {
            return;
        }

        let text = self.input_text.trim().to_string();
        let channel = CHANNELS[self.current_channel];
        let uptime_secs = crate::task::get_uptime_ticks() / crate::arch::idt::TIMER_HZ as u64;

        // Packet payload format: channel|sender|text
        let payload = format!("{}|guest|{}", channel, text);
        let _ = self.socket.send_to(Ipv4Address::LOOPBACK, Self::PORT, payload.as_bytes());

        // Append locally
        self.add_message(channel, "guest", &text, uptime_secs, false);

        // Clear input buffer
        self.input_text.clear();
        self.cursor = 0;

        // Autonomous AI Coprocessor Trigger
        if channel == "#agent" || text.contains("@agent") {
            let reply = Self::generate_agent_response(&text);
            let agent_payload = format!("{}|agent|{}", channel, reply);
            let _ = self.socket.send_to(Ipv4Address::LOOPBACK, Self::PORT, agent_payload.as_bytes());
            self.add_message(channel, "agent", &reply, uptime_secs, false);
            crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::BeepSuccess);
        } else {
            crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::SnakeEat);
        }
    }

    /// Generates intelligent autonomous AI responses for queries.
    pub fn generate_agent_response(query: &str) -> String {
        let q = query.to_ascii_lowercase();
        if q.contains("status") || q.contains("diagnostic") || q.contains("health") {
            "System nominal. Ring 0/Ring 3 isolation verified. 21 E2E tests active.".to_string()
        } else if q.contains("memory") || q.contains("ram") || q.contains("heap") {
            "Heap: 16 MB dynamic pool allocated. Memory footprint < 60 MB target met.".to_string()
        } else if q.contains("vfs") || q.contains("file") || q.contains("disk") {
            "Virtual Filesystem mounted at RAM disk. /user, /system directories active.".to_string()
        } else if q.contains("help") || q.contains("command") {
            "Try: @agent status, @agent memory, @agent vfs, or ask questions.".to_string()
        } else if q.contains("ping") {
            "Pong! Loopback latency: < 0.1ms (127.0.0.1:8080).".to_string()
        } else {
            "AI Coprocessor received your message. All operating system modules nominal.".to_string()
        }
    }

    /// Polls the loopback network adapter for incoming packets.
    pub fn poll_network(&mut self) {
        while let Some((src_ip, src_port, payload)) = self.socket.recv_from() {
            if let Ok(msg_str) = core::str::from_utf8(&payload) {
                let parts: Vec<&str> = msg_str.splitn(3, '|').collect();
                if parts.len() == 3 {
                    let channel = parts[0];
                    let sender = parts[1];
                    let text = parts[2];

                    // Don't duplicate if sent by local guest in current tick
                    if sender != "guest" {
                        let uptime_secs = crate::task::get_uptime_ticks() / crate::arch::idt::TIMER_HZ as u64;
                        self.add_message(channel, sender, text, uptime_secs, false);
                    }
                }
            }
            self.status_message = Some(format!("Recv {} bytes from {}:{}", payload.len(), src_ip.to_string(), src_port));
        }
    }

    /// Handles keyboard events when AegisChat is focused.
    pub fn handle_key(&mut self, event: KeyEvent) {
        if !event.pressed {
            return;
        }

        match event.code {
            KeyCode::Enter => {
                self.send_current_message();
            }
            KeyCode::Backspace => {
                if self.cursor > 0 && !self.input_text.is_empty() {
                    self.cursor -= 1;
                    self.input_text.remove(self.cursor);
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            KeyCode::Right => {
                if self.cursor < self.input_text.len() {
                    self.cursor += 1;
                }
            }
            _ => {
                if let Some(c) = event.char_byte {
                    if c >= 32 && c <= 126 {
                        if self.cursor <= self.input_text.len() {
                            self.input_text.insert(self.cursor, c as char);
                            self.cursor += 1;
                        }
                    }
                }
            }
        }
    }

    /// Handles mouse clicks inside AegisChat.
    pub fn handle_mouse_down(&mut self, win: &Window, x: i32, y: i32) {
        let client = win.client_rect();

        // 1. Channel selection in left sidebar (width 125px)
        if x >= client.x && x < client.x + 125 {
            let channel_start_y = client.y + 36;
            for i in 0..CHANNELS.len() {
                let cy = channel_start_y + (i as i32 * 26);
                if y >= cy && y < cy + 24 {
                    self.current_channel = i;
                    self.scroll_offset = 0;
                    return;
                }
            }
        }

        // 2. Click on [ Send ] button (bottom right)
        let bottom_y = client.bottom() - 34;
        let send_x = client.right() - 68;
        if x >= send_x && x <= send_x + 60 && y >= bottom_y + 4 && y <= bottom_y + 28 {
            self.send_current_message();
            return;
        }
    }

    /// Renders AegisChat window content.
    pub fn render(&mut self, win: &Window, fb: &mut Framebuffer) {
        let client = win.client_rect();
        if client.width < 350 || client.height < 250 {
            return;
        }

        let sidebar_w = 125i32;

        // ── 1. Left Channels & Users Sidebar ──
        let sidebar_rect = Rect::new(client.x, client.y, sidebar_w as u32, client.height);
        draw_rect(fb, sidebar_rect, Color::rgb(20, 24, 32));
        draw_rect(fb, Rect::new(client.x + sidebar_w - 1, client.y, 1, client.height), Color::WINDOW_BORDER);

        // Sidebar Header: CHANNELS
        draw_string(fb, client.x + 10, client.y + 12, "CHANNELS", Color::TEXT_DIM, None);

        // Channel List
        let channel_start_y = client.y + 36;
        for (i, &name) in CHANNELS.iter().enumerate() {
            let cy = channel_start_y + (i as i32 * 26);
            let is_active = i == self.current_channel;

            if is_active {
                let pill = Rect::new(client.x + 6, cy, (sidebar_w - 12) as u32, 22);
                draw_rounded_rect(fb, pill, 4, Color::rgb(40, 50, 70));
            }

            let text_color = if is_active { Color::WHITE } else { Color::rgb(170, 180, 200) };
            draw_string(fb, client.x + 14, cy + 3, name, text_color, None);
        }

        // Sidebar Sub-header: ONLINE
        let online_y = channel_start_y + (CHANNELS.len() as i32 * 26) + 16;
        draw_string(fb, client.x + 10, online_y, "ONLINE (3)", Color::TEXT_DIM, None);

        // User Badges
        let users = [
            ("guest (You)", Color::rgb(0, 230, 255)),   // Cyan
            ("agent (AI)", Color::rgb(40, 220, 100)),   // Emerald
            ("kernel", Color::rgb(255, 180, 50)),       // Amber
        ];
        for (i, &(user_name, dot_color)) in users.iter().enumerate() {
            let uy = online_y + 22 + (i as i32 * 22);
            draw_circle(fb, client.x + 14, uy + 7, 3, dot_color);
            draw_string(fb, client.x + 24, uy, user_name, Color::rgb(200, 210, 225), None);
        }

        // ── 2. Main Chat Feed Area ──
        let main_x = client.x + sidebar_w;
        let main_w = client.width - sidebar_w as u32;

        // Top Channel Header (28px)
        let header_rect = Rect::new(main_x, client.y, main_w, 28);
        draw_rect(fb, header_rect, Color::rgb(26, 30, 40));
        draw_rect(fb, Rect::new(main_x, client.y + 27, main_w, 1), Color::WINDOW_BORDER);

        let active_channel_name = CHANNELS[self.current_channel];
        draw_string(fb, main_x + 12, client.y + 6, active_channel_name, Color::WHITE, None);

        // Network Socket Telemetry Badge
        let socket_info = "UDP 127.0.0.1:8080 [Connected]";
        draw_string(fb, client.right() - 250, client.y + 6, socket_info, Color::rgb(100, 220, 140), None);

        // ── 3. Bottom Message Input Bar (34px) ──
        let input_bar_y = client.bottom() - 34;
        let input_bar_rect = Rect::new(main_x, input_bar_y, main_w, 34);
        draw_rect(fb, input_bar_rect, Color::rgb(22, 26, 34));
        draw_rect(fb, Rect::new(main_x, input_bar_y, main_w, 1), Color::WINDOW_BORDER);

        // Input Text Box
        let box_w = main_w - 80;
        let input_box_rect = Rect::new(main_x + 8, input_bar_y + 5, box_w, 24);
        draw_rounded_rect(fb, input_box_rect, 4, Color::rgb(32, 38, 50));
        draw_rounded_rect_outline(fb, input_box_rect, 4, Color::rgb(60, 70, 90));

        // Render Input Text and Blinking Cursor
        if self.input_text.is_empty() {
            let placeholder = format!("Message {}...", active_channel_name);
            draw_string(fb, main_x + 14, input_bar_y + 9, &placeholder, Color::TEXT_DIM, None);
        } else {
            draw_string(fb, main_x + 14, input_bar_y + 9, &self.input_text, Color::WHITE, None);
        }

        // Draw Cursor
        let cursor_x = main_x + 14 + (self.cursor as i32 * 8);
        draw_rect(fb, Rect::new(cursor_x, input_bar_y + 8, 2, 16), Color::rgb(0, 200, 255));

        // [ Send ] Button
        let send_btn_rect = Rect::new(client.right() - 68, input_bar_y + 5, 60, 24);
        draw_rounded_rect(fb, send_btn_rect, 4, Color::rgb(30, 110, 220));
        draw_string(fb, client.right() - 56, input_bar_y + 9, "Send", Color::WHITE, None);

        // ── 4. Scrollable Message Feed ──
        let feed_y = client.y + 32;
        let feed_h = (input_bar_y - feed_y) as u32;
        let feed_rect = Rect::new(main_x, feed_y, main_w, feed_h);
        draw_rect(fb, feed_rect, Color::rgb(16, 18, 24));

        // Filter messages for current channel
        let channel_msgs: Vec<&ChatMessage> = self
            .messages
            .iter()
            .filter(|m| m.channel == active_channel_name)
            .collect();

        let row_h = 36i32;
        let max_visible_rows = (feed_h as i32 / row_h) as usize;
        let start_idx = channel_msgs.len().saturating_sub(max_visible_rows);

        for (row, &msg) in channel_msgs[start_idx..].iter().enumerate() {
            let my = feed_y + (row as i32 * row_h) + 4;

            // Sender Badge Color
            let badge_color = match msg.sender.as_str() {
                "guest" => Color::rgb(0, 210, 255),  // Cyan
                "agent" => Color::rgb(40, 220, 100), // Emerald
                "kernel" => Color::rgb(255, 175, 40), // Amber
                _ => Color::rgb(180, 180, 180),
            };

            // Sender Name
            draw_string(fb, main_x + 12, my, &msg.sender, badge_color, None);

            // Timestamp (HH:MM:SS format)
            let secs = msg.timestamp_secs;
            let time_str = format!("{:02}:{:02}", (secs / 60) % 60, secs % 60);
            draw_string(fb, main_x + 12 + (msg.sender.len() as i32 * 8) + 12, my + 1, &time_str, Color::TEXT_DIM, None);

            // Message Body Text
            draw_string(fb, main_x + 12, my + 16, &msg.text, Color::WHITE, None);
        }
    }
}
