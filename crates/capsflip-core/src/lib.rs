//! Platform-independent logic of capsflip.
//!
//! Everything that decides lives here; the `capsflip` binary only translates
//! between Win32 and these types. See `docs/design.md`.

#![forbid(unsafe_code)]

pub mod engine;
pub mod key;
pub mod layout;
pub mod rules;
