//! What only `liblxb_toolkit` answers: its version, its marks, its named
//! lists. Each answers nothing (`None`, empty) without the toolkit.

use crate::page::c_string;
use crate::sys::{Symbols, load, static_str};
use std::ffi::{c_char, c_ulong};

/// The toolkit's version, such as `0.9.3`.
pub fn version() -> Option<&'static str> {
    let function = load().ok()?.sym.lxb_version?;
    // SAFETY: takes nothing, answers a static string.
    static_str(unsafe { function() })
}

/// One of the toolkit's named lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum List {
    Action,
    Role,
    Metric,
    Text,
    Surface,
    Overlay,
    Sound,
    /// The shell's marks, for `lxb_draw_icon` and a widget's `lxb_mark`.
    Glyph,
}

type Count = unsafe extern "C" fn() -> c_ulong;
type Name = unsafe extern "C" fn(c_ulong) -> *const c_char;

fn functions(sym: &Symbols, list: List) -> (Option<Count>, Option<Name>) {
    match list {
        List::Action => (sym.lxb_action_count, sym.lxb_action_name),
        List::Role => (sym.lxb_role_count, sym.lxb_role_name),
        List::Metric => (sym.lxb_metric_count, sym.lxb_metric_name),
        List::Text => (sym.lxb_text_count, sym.lxb_text_name),
        List::Surface => (sym.lxb_surface_count, sym.lxb_surface_name),
        List::Overlay => (sym.lxb_overlay_count, sym.lxb_overlay_name),
        List::Sound => (sym.lxb_sound_count, sym.lxb_sound_name),
        List::Glyph => (sym.lxb_glyph_count, sym.lxb_glyph_name),
    }
}

/// Every name of a list, in index order (`lxb_*_count`, `lxb_*_name`).
pub fn names(list: List) -> Option<Vec<&'static str>> {
    let lxb = load().ok()?;
    let (Some(count), Some(name)) = functions(&lxb.sym, list) else {
        return None;
    };
    // SAFETY: plain values in, static strings out.
    let count = unsafe { count() };
    (0..count)
        .map(|index| static_str(unsafe { name(index) }))
        .collect()
}

/// Whether `name` is one of the shell's marks.
pub fn has_glyph(name: &str) -> bool {
    names(List::Glyph).is_some_and(|names| names.contains(&name))
}

/// A named duration in seconds (`lxb_duration_named`), such as `panel`.
pub fn duration(name: &str) -> Option<f32> {
    let function = load().ok()?.sym.lxb_duration_named?;
    let name = c_string(name);
    // SAFETY: a valid string in, a plain value out.
    let seconds = unsafe { function(name.as_ptr()) };
    (seconds > 0.0).then_some(seconds)
}
