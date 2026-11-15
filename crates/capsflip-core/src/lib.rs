//! Platform-independent logic of capsflip.
//!
//! Everything that decides lives here; the `capsflip` binary only translates
//! between Win32 and these types. See `docs/design.md`.

#![forbid(unsafe_code)]

pub mod apps;
pub mod autostart;
pub mod config;
pub mod controller;
pub mod engine;
pub mod icon;
pub mod key;
pub mod layout;
pub mod logfile;
pub mod rules;
pub mod strokes;
pub mod tray;
