//! Integrated Core System Applications & Demo Suite for AegisOS
//!
//! Exposes all fourteen system applications: Crash-Test Demo, Activity Monitor,
//! Terminal Shell, Aegis Files, AegisPad, Aegis Browser, Minesweeper, AegisSynth,
//! AegisChat, Calculator, Snake, Aegis Paint, System Settings, and About Dialog.

pub mod about;
pub mod activity_monitor;
pub mod browser;
pub mod calculator;
pub mod chat;
pub mod crash_test;
pub mod editor;
pub mod file_manager;
pub mod minesweeper;
pub mod paint;
pub mod settings;
pub mod snake;
pub mod synth;
pub mod terminal;

pub use about::AboutDialogApp;
pub use activity_monitor::ActivityMonitorApp;
pub use browser::BrowserApp;
pub use calculator::CalculatorApp;
pub use chat::ChatApp;
pub use crash_test::{
    trigger_divide_by_zero, trigger_invalid_opcode, trigger_null_pointer, trigger_oob_write,
    CrashTestApp,
};
pub use editor::EditorApp;
pub use file_manager::{FileManagerAction, FileManagerApp};
pub use minesweeper::MinesweeperApp;
pub use paint::PaintApp;
pub use settings::{SettingsAction, SettingsApp, SettingsTab};
pub use snake::SnakeApp;
pub use synth::SynthApp;
pub use terminal::TerminalApp;

use crate::drivers::framebuffer::Framebuffer;
use crate::drivers::ps2_keyboard::KeyEvent;
use crate::gui::dock::AppId;
use crate::gui::menubar::WallpaperTheme;
use crate::gui::window::Window;

pub enum AppAction {
    None,
    CloseWindow,
    LaunchApp(AppId),
    FaultTriggered(usize),
    OpenFileInEditor(alloc::string::String),
    SetWallpaper(WallpaperTheme),
    SetCustomWallpaper(alloc::string::String),
}

/// Unified Application Suite state holding instances of all system applications.
pub struct AppSuite {
    pub crash_test: CrashTestApp,
    pub activity_monitor: ActivityMonitorApp,
    pub terminal: TerminalApp,
    pub file_manager: FileManagerApp,
    pub editor: EditorApp,
    pub browser: BrowserApp,
    pub minesweeper: MinesweeperApp,
    pub synth: SynthApp,
    pub chat: ChatApp,
    pub calculator: CalculatorApp,
    pub snake: SnakeApp,
    pub paint: PaintApp,
    pub settings: SettingsApp,
    pub about: AboutDialogApp,
}

impl AppSuite {
    pub fn new() -> Self {
        Self {
            crash_test: CrashTestApp::new(None),
            activity_monitor: ActivityMonitorApp::new(),
            terminal: TerminalApp::new(),
            file_manager: FileManagerApp::new(),
            editor: EditorApp::new(),
            browser: BrowserApp::new(),
            minesweeper: MinesweeperApp::new(),
            synth: SynthApp::new(),
            chat: ChatApp::new(),
            calculator: CalculatorApp::new(),
            snake: SnakeApp::new(),
            paint: PaintApp::new(),
            settings: SettingsApp::new(),
            about: AboutDialogApp::new(),
        }
    }

    /// Renders application content corresponding to the window's AppId.
    pub fn render_app(&mut self, win: &Window, fb: &mut Framebuffer) {
        match win.app_id {
            AppId::CrashTest => self.crash_test.render(win, fb),
            AppId::ActivityMonitor => self.activity_monitor.render(win, fb),
            AppId::Terminal => self.terminal.render(win, fb),
            AppId::FileManager => self.file_manager.render(win, fb),
            AppId::AegisPad => self.editor.render(win, fb),
            AppId::Browser => self.browser.render(win, fb),
            AppId::Minesweeper => self.minesweeper.render(win, fb),
            AppId::Synth => self.synth.render(win, fb),
            AppId::Chat => self.chat.render(win, fb),
            AppId::Calculator => self.calculator.render(win, fb),
            AppId::Snake => self.snake.render(win, fb),
            AppId::Paint => self.paint.render(win, fb),
            AppId::Settings => self.settings.render(win, fb),
            AppId::AboutDialog => self.about.render(win, fb),
            // Content for this window is produced by a Ring 3 process, which
            // draws into a shared surface the kernel only reads.
            AppId::UserTerminal => render_user_surface(win, fb),
        }
    }

    /// Dispatches mouse clicks to the active application.
    pub fn handle_mouse_down(&mut self, win: &Window, x: i32, y: i32, shift: bool) -> AppAction {
        match win.app_id {
            AppId::CrashTest => {
                if let Some(fault_idx) = self.crash_test.handle_click(win, x, y) {
                    AppAction::FaultTriggered(fault_idx)
                } else {
                    AppAction::None
                }
            }
            AppId::ActivityMonitor => {
                self.activity_monitor.handle_click(win, x, y);
                AppAction::None
            }
            AppId::Calculator => {
                self.calculator.handle_click(win, x, y);
                AppAction::None
            }
            AppId::Terminal => AppAction::None,
            AppId::Browser => {
                self.browser.handle_click(win, x, y);
                AppAction::None
            }
            AppId::Minesweeper => {
                self.minesweeper.handle_mouse_down(win, x, y, false, shift);
                AppAction::None
            }
            AppId::Synth => {
                self.synth.handle_mouse_down(win, x, y);
                AppAction::None
            }
            AppId::Chat => {
                self.chat.handle_mouse_down(win, x, y);
                AppAction::None
            }
            AppId::FileManager => {
                match self.file_manager.handle_click(win, x, y) {
                    FileManagerAction::OpenFileInEditor(path) => AppAction::OpenFileInEditor(path),
                    FileManagerAction::None => AppAction::None,
                }
            }
            AppId::AegisPad => {
                self.editor.handle_click(win, x, y);
                AppAction::None
            }
            AppId::Snake => AppAction::None,
            AppId::Paint => {
                self.paint.handle_mouse_down(win, x, y);
                AppAction::None
            }
            AppId::Settings => {
                match self.settings.handle_mouse_down(win, x, y) {
                    SettingsAction::SetTheme(theme) => AppAction::SetWallpaper(theme),
                    SettingsAction::SetCustomWallpaper(path) => AppAction::SetCustomWallpaper(path),
                    SettingsAction::None => AppAction::None,
                }
            }
            // A Ring 3 window takes no kernel-side click handling; the process
            // owns everything inside its client rect.
            AppId::UserTerminal => AppAction::None,
            AppId::AboutDialog => {
                if self.about.handle_click(win, x, y) {
                    AppAction::CloseWindow
                } else {
                    AppAction::None
                }
            }
        }
    }

    /// Dispatches right mouse button clicks to the active application.
    pub fn handle_mouse_down_right(&mut self, win: &Window, x: i32, y: i32) -> AppAction {
        match win.app_id {
            AppId::Minesweeper => {
                self.minesweeper.handle_mouse_down(win, x, y, true, false);
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    /// Dispatches mouse drag movements to the active application.
    pub fn handle_mouse_drag(&mut self, win: &Window, x: i32, y: i32) {
        if win.app_id == AppId::Paint {
            self.paint.handle_mouse_drag(win, x, y);
        }
    }

    /// Resets application mouse drag states on mouse up.
    pub fn handle_mouse_up(&mut self) {
        self.paint.handle_mouse_up();
        self.minesweeper.handle_mouse_up();
    }

    /// Dispatches keyboard typing events to the focused application.
    pub fn handle_key(&mut self, app_id: AppId, event: KeyEvent) -> Option<AppId> {
        match app_id {
            AppId::Terminal => self.terminal.handle_key(event),
            AppId::AegisPad => {
                self.editor.handle_key(event);
                None
            }
            AppId::Calculator => {
                self.calculator.handle_key(event);
                None
            }
            AppId::Snake => {
                self.snake.handle_key(event);
                None
            }
            AppId::Browser => {
                self.browser.handle_key(event);
                None
            }
            AppId::Minesweeper => {
                self.minesweeper.handle_key(event);
                None
            }
            AppId::Synth => {
                self.synth.handle_key(event);
                None
            }
            AppId::Chat => {
                self.chat.handle_key(event);
                None
            }
            _ => None,
        }
    }
}


/// Blits a Ring 3 process's window surface into its client rect.
///
/// Clipped to whichever is smaller, so a window larger than the surface shows
/// background around it and a smaller one shows the top-left corner. The kernel
/// only ever reads here: user code cannot make the compositor write anywhere.
fn render_user_surface(win: &Window, fb: &mut Framebuffer) {
    use crate::gui::surface::{
        pixel_from, snapshot_frames, SURFACE_FRAME_COUNT, SURFACE_HEIGHT, SURFACE_WIDTH,
    };
    use crate::memory::PhysAddr;

    let rect = win.client_rect();

    // Snapshot the frame list, then blit without holding the lock -- see
    // `surface::snapshot_frames` for why holding it here would deadlock.
    let mut frames = [PhysAddr::new(0); SURFACE_FRAME_COUNT];
    let count = snapshot_frames(&mut frames);
    if count == 0 {
        return;
    }
    let frames = &frames[..count];

    let width = core::cmp::min(rect.width as usize, SURFACE_WIDTH);
    let height = core::cmp::min(rect.height as usize, SURFACE_HEIGHT);

    for y in 0..height {
        for x in 0..width {
            let argb = pixel_from(frames, x, y);
            fb.draw_pixel(
                rect.x + x as i32,
                rect.y + y as i32,
                crate::gui::primitives::Color::from_u32(argb),
            );
        }
    }
}
