//! Aegis Files Interactive Graphical File Manager for AegisOS
//!
//! Features macOS Finder-style split-pane navigation, Places sidebar,
//! directory breadcrumbs, metadata columns (Name, Type, Size), directory creation,
//! file deletion, text file previews, and inter-app launching into AegisPad.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::drivers::framebuffer::Framebuffer;
use crate::fs::FileMetadata;
use crate::gui::font::{
    draw_mini_doc, draw_mini_folder, draw_mini_image, draw_string,
};
use crate::gui::primitives::{
    draw_rect, draw_rounded_rect, Color, Rect,
};
use crate::gui::window::Window;

pub enum FileManagerAction {
    None,
    OpenFileInEditor(String),
}

pub struct FileManagerApp {
    pub current_dir: String,
    pub items: Vec<FileMetadata>,
    pub selected_idx: Option<usize>,
    pub status_message: Option<String>,
    pub last_click_tick: u64,
}

impl FileManagerApp {
    pub fn new() -> Self {
        let current_dir = "/".to_string();
        let mut app = Self {
            current_dir,
            items: Vec::new(),
            selected_idx: None,
            status_message: Some("Ready — Browse VFS items".to_string()),
            last_click_tick: 0,
        };
        app.refresh();
        app
    }

    /// Refreshes the items list from the in-memory VFS, sorting directories first.
    pub fn refresh(&mut self) {
        let mut list = crate::fs::list_dir(&self.current_dir);
        list.sort_by(|a, b| match (a.is_directory, b.is_directory) {
            (true, false) => core::cmp::Ordering::Less,
            (false, true) => core::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });
        self.items = list;
        if let Some(idx) = self.selected_idx {
            if idx >= self.items.len() {
                self.selected_idx = None;
            }
        }
    }

    /// Navigates to a specific directory path.
    pub fn navigate_to(&mut self, path: &str) {
        self.current_dir = path.to_string();
        self.selected_idx = None;
        self.refresh();
        self.status_message = Some(format!("Navigated to {}", path));
    }

    /// Navigates up to the parent directory.
    pub fn navigate_up(&mut self) {
        if self.current_dir == "/" {
            return;
        }

        let parts: Vec<&str> = self.current_dir.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() <= 1 {
            self.navigate_to("/");
        } else {
            let parent = format!("/{}", parts[..parts.len() - 1].join("/"));
            self.navigate_to(&parent);
        }
    }

    /// Creates a new directory in the current path.
    pub fn create_folder(&mut self) {
        let mut counter = 1;
        let mut target_name = "new_folder".to_string();

        loop {
            let target_path = if self.current_dir == "/" {
                format!("/{}", target_name)
            } else {
                format!("{}/{}", self.current_dir, target_name)
            };

            if !crate::fs::file_exists(&target_path) {
                if crate::fs::create_dir(&target_path).is_ok() {
                    self.status_message = Some(format!("Created directory: {}", target_name));
                    self.refresh();
                } else {
                    self.status_message = Some("Error: Failed to create folder".to_string());
                }
                break;
            }

            counter += 1;
            target_name = format!("new_folder_{}", counter);
        }
    }

    /// Deletes the currently selected item from the VFS.
    pub fn delete_selected(&mut self) {
        if let Some(idx) = self.selected_idx {
            if idx < self.items.len() {
                let path = self.items[idx].path.clone();
                let name = self.items[idx].name.clone();
                if crate::fs::remove_file(&path).is_ok() {
                    self.status_message = Some(format!("Deleted: {}", name));
                    self.selected_idx = None;
                    self.refresh();
                } else {
                    self.status_message = Some("Error: Could not delete item".to_string());
                }
            }
        }
    }

    /// Formats byte sizes into human-readable strings.
    fn format_size(bytes: usize, is_dir: bool) -> String {
        if is_dir {
            "—".to_string()
        } else if bytes < 1024 {
            format!("{} B", bytes)
        } else if bytes < 1024 * 1024 {
            format!("{:.1} KB", bytes as f32 / 1024.0)
        } else {
            format!("{:.1} MB", bytes as f32 / (1024.0 * 1024.0))
        }
    }

    /// Detects file kind / description from name and attributes.
    fn file_type_desc(name: &str, is_dir: bool) -> &'static str {
        if is_dir {
            "Folder"
        } else if name.ends_with(".txt") {
            "Text Document"
        } else if name.ends_with(".ppm") {
            "PPM Image"
        } else if name == "os_release" {
            "System Release"
        } else {
            "Plain File"
        }
    }

    /// Handles mouse clicks inside the File Manager window.
    pub fn handle_click(&mut self, win: &Window, x: i32, y: i32) -> FileManagerAction {
        let client = win.client_rect();
        let rel_x = x - client.x;
        let rel_y = y - client.y;

        // 1. Top Navigation Toolbar (y: 0..28)
        if (0..28).contains(&rel_y) {
            // [ < Back ] (8..54)
            if (8..54).contains(&rel_x) {
                self.navigate_up();
                return FileManagerAction::None;
            }
            // [ / Root ] (58..110)
            if (58..110).contains(&rel_x) {
                self.navigate_to("/");
                return FileManagerAction::None;
            }
            // [ /user ] (114..166)
            if (114..166).contains(&rel_x) {
                self.navigate_to("/user");
                return FileManagerAction::None;
            }
            // [ /system ] (170..232)
            if (170..232).contains(&rel_x) {
                self.navigate_to("/system");
                return FileManagerAction::None;
            }
            // [ + Folder ] (236..308)
            if (236..308).contains(&rel_x) {
                self.create_folder();
                return FileManagerAction::None;
            }
        }

        // 2. Left Places Sidebar (x: 0..120, y: 28..client.height - 28)
        let sidebar_w = 120;
        let content_h = client.height as i32 - 28 - 28;
        if (0..sidebar_w).contains(&rel_x) && (28..28 + content_h).contains(&rel_y) {
            let item_y0 = 48;
            let item_h = 24;

            // Place 1: Root (/)
            if (item_y0..item_y0 + item_h).contains(&rel_y) {
                self.navigate_to("/");
                return FileManagerAction::None;
            }
            // Place 2: User (/user)
            if (item_y0 + item_h..item_y0 + 2 * item_h).contains(&rel_y) {
                self.navigate_to("/user");
                return FileManagerAction::None;
            }
            // Place 3: System (/system)
            if (item_y0 + 2 * item_h..item_y0 + 3 * item_h).contains(&rel_y) {
                self.navigate_to("/system");
                return FileManagerAction::None;
            }
        }

        // 3. Right File Browser Area (x: 120..client.width, y: 48..client.height - 28)
        let list_y0 = 48;
        let row_h = 22;
        if rel_x >= sidebar_w && rel_y >= list_y0 && rel_y < client.height as i32 - 28 {
            let row_idx = ((rel_y - list_y0) / row_h) as usize;
            if row_idx < self.items.len() {
                // Check if clicking already selected item (Double-click / Open)
                if self.selected_idx == Some(row_idx) {
                    let item = &self.items[row_idx];
                    if item.is_directory {
                        let target = item.path.clone();
                        self.navigate_to(&target);
                        return FileManagerAction::None;
                    } else if item.name.ends_with(".txt") || item.name == "os_release" {
                        return FileManagerAction::OpenFileInEditor(item.path.clone());
                    }
                } else {
                    self.selected_idx = Some(row_idx);
                    let item = &self.items[row_idx];
                    self.status_message = Some(format!("Selected: {}", item.name));
                }
                return FileManagerAction::None;
            }
        }

        // 4. Bottom Action Bar (y: client.height - 28..client.height)
        if rel_y >= client.height as i32 - 28 {
            // [ Open ] button (client.width - 130 .. client.width - 70)
            let open_x1 = client.width as i32 - 130;
            let open_x2 = client.width as i32 - 72;
            if (open_x1..open_x2).contains(&rel_x) {
                if let Some(idx) = self.selected_idx {
                    if idx < self.items.len() {
                        let item = &self.items[idx];
                        if item.is_directory {
                            let target = item.path.clone();
                            self.navigate_to(&target);
                            return FileManagerAction::None;
                        } else if item.name.ends_with(".txt") || item.name == "os_release" {
                            return FileManagerAction::OpenFileInEditor(item.path.clone());
                        }
                    }
                }
            }

            // [ Delete ] button (client.width - 66 .. client.width - 8)
            let del_x1 = client.width as i32 - 66;
            let del_x2 = client.width as i32 - 8;
            if (del_x1..del_x2).contains(&rel_x) {
                self.delete_selected();
                return FileManagerAction::None;
            }
        }

        FileManagerAction::None
    }

    /// Renders the complete Aegis Files interface.
    pub fn render(&self, win: &Window, fb: &mut Framebuffer) {
        let client = win.client_rect();
        if client.width < 340 || client.height < 240 {
            return;
        }

        // 1. Top Action Toolbar (y: 0..28)
        let bar_h = 28;
        let bar_rect = Rect::new(client.x, client.y, client.width, bar_h);
        draw_rect(fb, bar_rect, Color::rgb(32, 36, 44));
        draw_rect(fb, Rect::new(client.x, client.y + bar_h as i32 - 1, client.width, 1), Color::WINDOW_BORDER);

        // Buttons
        // [ < Back ]
        let back_color = if self.current_dir != "/" { Color::BUTTON_BG } else { Color::rgb(40, 44, 52) };
        draw_rounded_rect(fb, Rect::new(client.x + 8, client.y + 4, 46, 20), 3, back_color);
        draw_string(fb, client.x + 14, client.y + 6, "< Up", Color::WHITE, None);

        // [ / Root ]
        draw_rounded_rect(fb, Rect::new(client.x + 58, client.y + 4, 52, 20), 3, Color::BUTTON_BG);
        draw_string(fb, client.x + 64, client.y + 6, "/ Root", Color::WHITE, None);

        // [ /user ]
        draw_rounded_rect(fb, Rect::new(client.x + 114, client.y + 4, 52, 20), 3, Color::BUTTON_BG);
        draw_string(fb, client.x + 120, client.y + 6, "/user", Color::WHITE, None);

        // [ /system ]
        draw_rounded_rect(fb, Rect::new(client.x + 170, client.y + 4, 62, 20), 3, Color::BUTTON_BG);
        draw_string(fb, client.x + 176, client.y + 6, "/system", Color::WHITE, None);

        // [ + Folder ]
        draw_rounded_rect(fb, Rect::new(client.x + 236, client.y + 4, 72, 20), 3, Color::rgb(45, 100, 160));
        draw_string(fb, client.x + 242, client.y + 6, "+ Folder", Color::WHITE, None);

        // Current Path Breadcrumb Pill
        let path_text = format!("Path: {}", self.current_dir);
        draw_string(fb, client.x + 316, client.y + 7, &path_text, Color::TEXT_DIM, None);

        // 2. Left Places Sidebar (width: 120)
        let sidebar_w = 120;
        let body_y = client.y + bar_h as i32;
        let body_h = client.height - bar_h - 28;
        let sidebar_rect = Rect::new(client.x, body_y, sidebar_w, body_h);
        draw_rect(fb, sidebar_rect, Color::rgb(22, 25, 32));
        draw_rect(fb, Rect::new(client.x + sidebar_w as i32 - 1, body_y, 1, body_h), Color::WINDOW_BORDER);

        // Header: PLACES
        draw_string(fb, client.x + 10, body_y + 8, "PLACES", Color::rgb(120, 130, 150), None);

        // Sidebar Items
        let places = [
            ("/", "Root (/)"),
            ("/user", "User (/user)"),
            ("/system", "System (/sys)"),
        ];

        let place_y0 = body_y + 26;
        for (i, &(target, label)) in places.iter().enumerate() {
            let py = place_y0 + (i as i32 * 24);
            let is_active = self.current_dir == target;
            if is_active {
                draw_rounded_rect(fb, Rect::new(client.x + 4, py - 2, sidebar_w - 8, 20), 3, Color::rgba(70, 140, 240, 60));
            }
            draw_mini_folder(fb, client.x + 8, py + 2);
            let color = if is_active { Color::WHITE } else { Color::TEXT_DIM };
            draw_string(fb, client.x + 24, py + 1, label, color, None);
        }

        // Sidebar Separator & STORAGE Stats
        let stats_y = body_y + 110;
        draw_rect(fb, Rect::new(client.x + 10, stats_y - 8, sidebar_w - 20, 1), Color::WINDOW_BORDER);
        draw_string(fb, client.x + 10, stats_y, "STORAGE", Color::rgb(120, 130, 150), None);

        let (total_files, total_bytes) = crate::fs::get_fs_stats();
        let files_str = format!("{} files", total_files);
        let bytes_str = format!("{:.1} KB", total_bytes as f32 / 1024.0);
        draw_string(fb, client.x + 10, stats_y + 18, &files_str, Color::TEXT_DIM, None);
        draw_string(fb, client.x + 10, stats_y + 34, &bytes_str, Color::TEXT_DIM, None);

        // 3. Right File Browser Area
        let list_x = client.x + sidebar_w as i32;
        let list_w = client.width - sidebar_w;
        draw_rect(fb, Rect::new(list_x, body_y, list_w, body_h), Color::rgb(18, 20, 26));

        // Column Headers (y: body_y .. body_y + 20)
        let header_h = 20;
        draw_rect(fb, Rect::new(list_x, body_y, list_w, header_h), Color::rgb(26, 29, 36));
        draw_rect(fb, Rect::new(list_x, body_y + header_h as i32 - 1, list_w, 1), Color::WINDOW_BORDER);

        let col_name_x = list_x + 12;
        let col_type_x = list_x + 140;
        let col_size_x = list_x + 290;

        draw_string(fb, col_name_x, body_y + 3, "Name", Color::TEXT_DIM, None);
        draw_string(fb, col_type_x, body_y + 3, "Kind", Color::TEXT_DIM, None);
        draw_string(fb, col_size_x, body_y + 3, "Size", Color::TEXT_DIM, None);

        // File Rows
        let row_y0 = body_y + header_h as i32;
        let row_h = 22;

        for (i, item) in self.items.iter().enumerate() {
            let ry = row_y0 + (i as i32 * row_h);
            if ry + row_h > body_y + body_h as i32 {
                break;
            }

            let is_selected = self.selected_idx == Some(i);

            // Row background highlight
            if is_selected {
                draw_rect(fb, Rect::new(list_x, ry, list_w, row_h as u32), Color::rgb(35, 95, 185));
            } else if i % 2 == 1 {
                draw_rect(fb, Rect::new(list_x, ry, list_w, row_h as u32), Color::rgba(255, 255, 255, 5));
            }

            // File Icon
            let icon_x = col_name_x;
            let icon_y = ry + 5;
            if item.is_directory {
                draw_mini_folder(fb, icon_x, icon_y);
            } else if item.name.ends_with(".ppm") {
                draw_mini_image(fb, icon_x, icon_y);
            } else {
                draw_mini_doc(fb, icon_x, icon_y);
            }

            // Name
            let name_color = if is_selected { Color::WHITE } else { Color::TEXT_PRIMARY };
            draw_string(fb, col_name_x + 18, ry + 3, &item.name, name_color, None);

            // Kind / Type
            let kind_str = Self::file_type_desc(&item.name, item.is_directory);
            let kind_color = if is_selected { Color::rgb(220, 235, 255) } else { Color::TEXT_DIM };
            draw_string(fb, col_type_x, ry + 3, kind_str, kind_color, None);

            // Size
            let size_str = Self::format_size(item.size_bytes, item.is_directory);
            let size_color = if is_selected { Color::rgb(220, 235, 255) } else { Color::TEXT_DIM };
            draw_string(fb, col_size_x, ry + 3, &size_str, size_color, None);
        }

        // 4. Bottom Action & Status Bar (height 28)
        let status_y = client.bottom() - 28;
        let status_rect = Rect::new(client.x, status_y, client.width, 28);
        draw_rect(fb, status_rect, Color::rgb(26, 29, 36));
        draw_rect(fb, Rect::new(client.x, status_y, client.width, 1), Color::WINDOW_BORDER);

        // Status text / item details
        let default_status = format!("{} items in {}", self.items.len(), self.current_dir);
        let status_txt = self.status_message.as_deref().unwrap_or(&default_status);
        draw_string(fb, client.x + 12, status_y + 6, status_txt, Color::TEXT_DIM, None);

        // Action buttons if item selected
        if let Some(idx) = self.selected_idx {
            if idx < self.items.len() {
                let open_x1 = client.width as i32 - 130;
                let del_x1 = client.width as i32 - 66;

                // [ Open ] button
                draw_rounded_rect(fb, Rect::new(client.x + open_x1, status_y + 4, 58, 20), 3, Color::rgb(35, 125, 65));
                draw_string(fb, client.x + open_x1 + 10, status_y + 6, "Open", Color::WHITE, None);

                // [ Delete ] button
                draw_rounded_rect(fb, Rect::new(client.x + del_x1, status_y + 4, 58, 20), 3, Color::rgb(170, 45, 45));
                draw_string(fb, client.x + del_x1 + 6, status_y + 6, "Delete", Color::WHITE, None);
            }
        }
    }
}
