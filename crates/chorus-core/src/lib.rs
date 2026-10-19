//! Chorus 2.0 pure logic.
//!
//! Everything here is computed from its inputs: no IO, no Tauri, no filesystem, no network, no
//! SQLite, no clock and no environment (ARCHITECTURE §2, D11). `tests/purity.rs` enforces it.

#![forbid(unsafe_code)]

pub mod activity;
pub mod agent;
pub mod batch;
pub mod build;
pub mod claude;
pub mod cli;
pub mod codex;
pub mod conpty;
pub mod env;
pub mod events;
pub mod hooks;
pub mod launch;
pub mod lock;
pub mod migrate;
pub mod opencode;
pub mod providers;
pub mod scrollback;
pub mod scrub;
pub mod session;
pub mod shim;
pub mod tree;
