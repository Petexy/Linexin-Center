//! The two ends of `LxbFrameV1`: a widget reading the frame the Center hands
//! it, and the Center building one.

use crate::page::Page;
use crate::types::Action;
use linexin_plugin_api::{ABI_VERSION, LXB_PAGE_LEAVE, LXB_PAGE_STAY, LxbFrameV1};
use std::ffi::c_int;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// A frame of a widget's lxb page, read.
pub struct WidgetFrame<'frame> {
    pub page: Page<'frame>,
    /// Where the page goes (inside its pane's padding).
    pub rect: [f32; 4],
    /// Whether the page has the light; without it, it is a preview.
    pub focused: bool,
    /// The page's actions this frame (empty unless focused).
    pub actions: Vec<Action>,
}

impl<'frame> WidgetFrame<'frame> {
    /// Validate and read the frame an `lxb_page` callback received. `None`
    /// for an incompatible frame, a null page, or a process that cannot open
    /// `liblxb_app.so`.
    ///
    /// # Safety
    /// `frame` must be the pointer the host passed to `lxb_page`, and the
    /// result must not outlive that call.
    pub unsafe fn read(frame: *const LxbFrameV1) -> Option<Self> {
        // SAFETY: guaranteed by the caller.
        let frame = unsafe { LxbFrameV1::read(frame) }?;
        // SAFETY: the host hands over its live page for this call.
        let page = unsafe { Page::from_raw(frame.page) }?;
        Some(Self {
            page,
            rect: frame.rect,
            focused: frame.focused,
            actions: Action::from_raw_list(frame.actions()),
        })
    }
}

/// `LXB_PAGE_LEAVE` when `leave`, otherwise `LXB_PAGE_STAY`.
pub fn page_result(leave: bool) -> u32 {
    if leave { LXB_PAGE_LEAVE } else { LXB_PAGE_STAY }
}

/// Run `body`, answering `fallback` if it panics: for every `extern "C"`
/// callback, which no panic may unwind out of.
pub fn guard<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(fallback)
}

/// The actions as the C indices a frame carries.
pub fn raw_actions(actions: &[Action]) -> Vec<c_int> {
    actions.iter().map(|action| action.raw() as c_int).collect()
}

/// The frame a host hands a widget's `lxb_page`. `actions` (from
/// [`raw_actions`]) must outlive the call it is passed to.
pub fn host_frame(page: &Page, rect: [f32; 4], focused: bool, actions: &[c_int]) -> LxbFrameV1 {
    LxbFrameV1 {
        abi_version: ABI_VERSION,
        struct_size: size_of::<LxbFrameV1>() as u32,
        page: page.as_raw(),
        rect,
        focused,
        actions: if actions.is_empty() {
            std::ptr::null()
        } else {
            actions.as_ptr()
        },
        action_count: actions.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_becomes_the_fallback() {
        assert_eq!(
            guard(LXB_PAGE_STAY, || panic!("drawn badly")),
            LXB_PAGE_STAY
        );
        assert_eq!(guard(0, || 7), 7);
    }

    #[test]
    fn actions_cross_as_their_indices() {
        let raw = raw_actions(&[Action::Down, Action::Back]);
        assert_eq!(raw, vec![3, 5]);
        assert_eq!(
            Action::from_raw_list(&raw),
            vec![Action::Down, Action::Back]
        );
        assert_eq!(page_result(true), LXB_PAGE_LEAVE);
    }
}
