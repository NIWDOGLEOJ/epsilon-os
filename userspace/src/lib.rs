#![no_std]

//! Shared runtime for AegisOS Ring 3 programs.
//!
//! Everything here is usable by any user process: the syscall shims, the window
//! surface, text rendering, and the allocation-free formatting helpers the
//! programs need because there is no heap in Ring 3.
//!
//! `rt.rs` is deliberately *not* part of this library. It defines `_start` and
//! the panic handler, which every binary needs exactly one of; including it per
//! binary keeps those symbols out of an rlib where `--gc-sections` might not
//! consider them reachable.

pub mod font;
pub mod surface;
pub mod sys;
pub mod text;
