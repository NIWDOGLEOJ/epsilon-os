//! AegisOS E2E Test Harness Module Index
//!
//! Provides types, memory/paging simulators, privilege/IDT/TSS, scheduler/PCB,
//! double-buffered framebuffer compositor, input decoders, window manager,
//! and 5 system applications.

pub mod types;
pub mod memory_sim;
pub mod privilege_sim;
pub mod scheduler_sim;
pub mod gui_sim;
pub mod input_sim;
pub mod wm_sim;
pub mod apps_sim;
pub mod os_kernel_env;

pub use types::*;
pub use memory_sim::*;
pub use privilege_sim::*;
pub use scheduler_sim::*;
pub use gui_sim::*;
pub use input_sim::*;
pub use wm_sim::*;
pub use apps_sim::*;
pub use os_kernel_env::*;
