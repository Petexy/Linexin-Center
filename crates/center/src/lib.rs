//! Linexin Center: a GTK4/libadwaita control center that hosts native (`.so`)
//! and legacy Python widgets from `/usr/share/linexin/widgets/`.
//!
//! The library exists so the GUI test (`tests/gui.rs`) can drive the real
//! window; the `linexin-center` binary only calls [`app::run`].

pub mod app;
pub mod catalog;
pub mod config;
pub mod host_api;
pub mod lxb_front;
pub mod native;
pub mod options;
pub mod paths;
mod shortcut;
pub mod window;
