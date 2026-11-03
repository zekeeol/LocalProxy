//! Views of the application shell.
//!
//! Each module owns one region of the window. The shell (`AppShell`) holds the
//! data; these functions only render it and dispatch interactions through
//! `cx.listener(...)`.

pub mod detail;
pub mod editor;
pub mod empty;
pub mod overlays;
pub mod settings;
pub mod sidebar;
pub mod titlebar;
