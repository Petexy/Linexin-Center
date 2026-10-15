//! Binding to LineXinBar's lxb-toolkit (`lxb_app.h`, and the parts of
//! `lxb_toolkit.h` an application needs), shared by the Center's LineXinBar
//! front end and every native widget's lxb page.
//!
//! # Loading
//!
//! Nothing links the libraries. [`load`] opens `liblxb_app.so` and
//! `liblxb_toolkit.so` with `dlopen` the first time it is called, keeps them
//! for the life of the process, and answers the same [`Lxb`] after; a widget
//! library calling it reaches the same instance the Center opened. Without
//! LineXinBar it fails with a sentence saying what is missing, and the rest
//! of the program carries on. Every function is resolved on its own: one an
//! older library lacks makes its wrapper do nothing and answer a neutral
//! value ([`Lxb::has`] asks first; `lxb_page_hovered`, `lxb_page_pointer` and
//! `lxb_page_pointer_moved`, `lxb_draw_picture`, `lxb_page_taking_text`,
//! `lxb_page_typed`, `lxb_page_board` and `lxb_page_light_shaped` are
//! lxb-toolkit 0.9.4's, which 0.9.3 lacks).
//!
//! # The window and the page
//!
//! [`App`] is `lxb_app`: `App::new(id, title)?.plain().driven()`, then
//! [`App::run`] with a closure drawing each frame, or [`App::shot`] for one
//! settled frame written to a PNG with no display. The closure gets a
//! [`Page`], a copyable handle valid for that frame, with one method per
//! `lxb_page_*`/`lxb_draw_*` function (same names without the prefix;
//! drawing calls keep `draw_`). Strings go in as `&str` (an interior NUL is
//! dropped), paths as `AsRef<Path>`, answers come back owned; rectangles are
//! `[x, y, width, height]`; enumerations are typed ([`Action`], [`Metric`],
//! [`Text`], [`Role`], [`Sound`], [`Press`] ...). A widget turns the page in
//! its `LxbFrameV1` into one with [`Page::from_raw`] or, with the rest of the
//! frame, [`WidgetFrame::read`]. No panic crosses into C: `run`/`shot` catch
//! one in the closure, close the window and resume it afterwards, and
//! [`guard`] does the same for a widget's own callbacks.
//!
//! # Pages made of flow controls
//!
//! [`Flow`] is what most widget pages are. Kept across frames, it takes the
//! frame's actions with [`Flow::input`] (Up/Down walk the light over the
//! controls drawn last frame with the Move sound, silence where it cannot
//! move; Accept/Submit press the lit control with the Press sound; Back, and
//! Left, come back as [`Leave`]; the rest as `unhandled`), then lays the page
//! out with [`Flow::begin`] → [`FlowFrame`]: `head` (the page's heading,
//! pinned at its top), `title`, `heading`, `text`, `note`, `pair`, `rule`,
//! `gap`, `space` and the controls `row`,
//! `row_value`, `item`, `entry`, `button`, each answering `true` once when
//! pressed by Accept, Submit or the pointer. Sizes are the library flow's own
//! (a test compares the two pictures pixel for pixel). Content longer than
//! the page scrolls, gliding to keep the light in view; what scrolled away
//! is left out and the ends are softened; Up past the first control lets go
//! of the light to read what is above it, and Down brings it back. As a
//! preview (`lit == false`) it draws no light and takes no input.
//!
//! The controls are drawn with `lxb_draw_*`, numbered with spots
//! ([`FLOW_SPOTS`]`+n`) and lit with the page's own light (`lxb_page_glide`)
//! rather than with `lxb_page_row` and `lxb_page_focus`: the library's flow
//! light cannot be hidden for a preview, and its numbering shifts when
//! scrolled-away controls are left out. Do not draw the library's own flow
//! controls on a frame a `Flow` is lit.
//!
//! ```ignore
//! unsafe extern "C" fn lxb_page(state: *mut c_void, frame: *const LxbFrameV1) -> u32 {
//!     linexin_lxb::guard(LXB_PAGE_STAY, || {
//!         let state = unsafe { &mut *state.cast::<State>() };
//!         let Some(frame) = (unsafe { WidgetFrame::read(frame) }) else { return LXB_PAGE_STAY };
//!         let input = state.flow.input(&frame.page, &frame.actions);
//!         let mut page = state.flow.begin(&frame.page, frame.rect, frame.focused);
//!         page.head("setting-info", &tr("System Information"));
//!         page.pair(&tr("Kernel"), &state.kernel);
//!         if page.button(&tr("Copy")) { state.copy() }
//!         page.finish();
//!         page_result(input.leave.is_some())
//!     })
//! }
//! ```
//!
//! # The host's screen
//!
//! The screen is laid out as LineXinBar's Software Hub (DistriBumpy) lays
//! out its own: one pane over the wallpaper, the pages' column as a sidebar
//! card at its left, the chosen page beside it, and the legend along the
//! foot. [`Column`] draws the card (an entry per page: mark at the size a
//! list uses, label, optional count). Its [`Column::input`] walks it while it
//! has the light, enters the page on Accept, Submit or Right, reports Back,
//! and takes the pointer: hovering chooses (without scrolling the list under
//! it), a click on the chosen entry enters it, a click while the page has the
//! light takes the light back. [`Split`] answers where the window's pane, the
//! card, the page and its foot stand ([`Panes`]), side by side or sliding as
//! one view on a window too narrow for both ([`Beside`]/[`Slide`] are the
//! toolkit's layout underneath). The column lights its entry with the same
//! page light the flows use, so the one light travels from the column into
//! the page and back; while the page has it the column keeps a quiet capsule
//! ([`QUIET`]). [`Legend`] writes what the buttons do as pictures of them
//! (pad or keyboard, as [`Hands`] follows), each pair a click target.
//!
//! A frame of the host, as `examples/common/mod.rs` writes it: read
//! [`Page::actions`] (and add the button of a legend pair that was clicked);
//! give them to [`Column::input`] while the column has the light (its
//! `for_page` are what followed an Enter); a click on [`PAGE_SPOT`] (written
//! over a preview page) enters it too; lay out with [`Split::layout`]; draw
//! the window's pane, the column, and call the widget with [`host_frame`] on
//! [`Panes::content`]; on `LXB_PAGE_LEAVE` take the light back with
//! [`hand_back_sound`]; draw the legend last on [`Panes::foot`]. Back in the
//! column asks whether to leave ([`Page::ask`]) and quits on the first answer.
//!
//! Spot numbers: flows use [`FLOW_SPOTS`]`+n`, the column [`COLUMN_SPOTS`]`+n`,
//! a host's preview page [`PAGE_SPOT`], the legend [`LEGEND_SPOTS`]`+n`; keep
//! a page's own spots clear of them (the tours use `0x200`).

mod app;
mod column;
mod emblem;
mod face;
mod flow;
mod layout;
mod legend;
mod list;
mod page;
mod side_panel;
mod split;
mod sys;
pub mod toolkit;
mod typed;
mod types;
mod widget;
pub mod wrap;

pub use app::App;
pub use column::{
    COLUMN_SPOTS, Column, ColumnContent, ColumnEntry, ColumnInput, QUIET, hand_back_sound,
};
pub use emblem::{Emblem, icon_file};
pub use flow::{FLOW_SPOTS, FieldInput, Flow, FlowFrame, Input, Leave};
pub use layout::{Beside, Slide, glide, peek};
pub use legend::{Button, Hands, Hint, LEGEND_SPOTS, Legend};
pub use page::Page;
pub use side_panel::{PANEL_SPOTS, SidePanel};
pub use split::{PAGE_SPOT, Panes, Split};
pub use sys::{APP_LIBRARY, Lxb, TOOLKIT_LIBRARY, load};
pub use typed::{TYPED_VARIABLE, apply as apply_typed};
pub use types::{
    Action, Align, Fit, Hold, Metric, Overlay, PickerSelection, Press, Role, Sound, Surface, Text,
};
pub use widget::{WidgetFrame, guard, host_frame, page_result, raw_actions};

/// Whether LineXinBar's libraries can be opened in this process.
pub fn available() -> bool {
    load().is_ok()
}

/// Whether this process runs in a LineXinBar session (`XDG_CURRENT_DESKTOP`
/// names `LineXinBar`, compared without case, among `:`-separated names).
pub fn in_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktops| names_linexinbar(&desktops))
}

fn names_linexinbar(desktops: &str) -> bool {
    desktops
        .split(':')
        .any(|desktop| desktop.trim().eq_ignore_ascii_case("LineXinBar"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_session_is_named_among_others_and_without_case() {
        assert!(super::names_linexinbar("LineXinBar"));
        assert!(super::names_linexinbar("KDE:linexinbar"));
        assert!(!super::names_linexinbar("KDE"));
        assert!(!super::names_linexinbar("LineXinBarX"));
        assert!(!super::names_linexinbar(""));
    }
}
