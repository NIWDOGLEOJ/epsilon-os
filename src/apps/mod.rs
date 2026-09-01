//! Integrated Core System Applications & Demo Suite for AegisOS
//!
//! Exposes Crash-Test Demo, Activity Monitor, Terminal Shell, AegisPad,
//! Calculator, Snake Arcade Game, and About Dialog.

pub mod about;
pub mod activity_monitor;
pub mod calculator;
pub mod crash_test;
pub mod editor;
pub mod snake;
pub mod terminal;

pub use about::AboutDialogApp;
pub use activity_monitor::ActivityMonitorApp;
pub use calculator::CalculatorApp;
pub use crash_test::{
    trigger_divide_by_zero, trigger_invalid_opcode, trigger_null_pointer, trigger_oob_write,
    CrashTestApp,
};
pub use editor::EditorApp;
pub use snake::SnakeApp;
pub use terminal::TerminalApp;

use crate::drivers::framebuffer::Framebuffer;
use crate::drivers::ps2_keyboard::KeyEvent;
use crate::gui::dock::AppId;
use crate::gui::window::Window;

pub enum AppAction {
    None,
    CloseWindow,
    LaunchApp(AppId),
    FaultTriggered(usize),
}

/// Unified Application Suite state holding instances of all system applications.
pub struct AppSuite {
    pub crash_test: CrashTestApp,
    pub activity_monitor: ActivityMonitorApp,
    pub terminal: TerminalApp,
    pub editor: EditorApp,
    pub calculator: CalculatorApp,
    pub snake: SnakeApp,
    pub about: AboutDialogApp,
}

impl AppSuite {
    pub fn new() -> Self {
        Self {
            crash_test: CrashTestApp::new(None),
            activity_monitor: ActivityMonitorApp::new(),
            terminal: TerminalApp::new(),
            editor: EditorApp::new(),
            calculator: CalculatorApp::new(),
            snake: SnakeApp::new(),
            about: AboutDialogApp::new(),
        }
    }

    /// Renders application content corresponding to the window's AppId.
    pub fn render_app(&mut self, win: &Window, fb: &mut Framebuffer) {
        match win.app_id {
            AppId::CrashTest => self.crash_test.render(win, fb),
            AppId::ActivityMonitor => self.activity_monitor.render(win, fb),
            AppId::Terminal => self.terminal.render(win, fb),
            AppId::AegisPad => self.editor.render(win, fb),
            AppId::Calculator => self.calculator.render(win, fb),
            AppId::Snake => self.snake.render(win, fb),
            AppId::AboutDialog => self.about.render(win, fb),
        }
    }

    /// Dispatches mouse clicks to the active application.
    pub fn handle_mouse_down(&mut self, win: &Window, x: i32, y: i32) -> AppAction {
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
            AppId::AegisPad => AppAction::None,
            AppId::Snake => AppAction::None,
            AppId::AboutDialog => {
                if self.about.handle_click(win, x, y) {
                    AppAction::CloseWindow
                } else {
                    AppAction::None
                }
            }
        }
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
            _ => None,
        }
    }
}
