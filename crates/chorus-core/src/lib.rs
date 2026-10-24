//! Chorus 2.0 pure logic.
//!
//! Everything here is computed from its inputs: no IO, no Tauri, no filesystem, no network, no
//! SQLite, no clock and no environment (ARCHITECTURE §2, D11). `tests/purity.rs` enforces it.

#![forbid(unsafe_code)]

pub mod activity;
pub mod narrator;
pub mod actions;
pub mod composer;
pub mod replies;
pub mod agent;
pub mod attention;
pub mod inbox;
pub mod badge;
pub mod strip;
pub mod export;
pub mod batch;
pub mod build;
pub mod claude;
pub mod cli;
pub mod codex;
pub mod conpty;
pub mod env;
pub mod events;
pub mod folders;
pub mod gemini;
pub mod git_status;
pub mod hooks;
pub mod launch;
pub mod lock;
pub mod migrate;
pub mod names;
pub mod opencode;
pub mod prefs;
pub mod projects;
pub mod providers;
pub mod routing;
pub mod scrollback;
pub mod scrub;
pub mod session;
pub mod shim;
pub mod tags;
pub mod tree;
pub mod worktree;
pub mod attachments;
pub mod progress;
pub mod codex_protocol;
pub mod codex_stream;
