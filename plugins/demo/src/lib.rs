//! Minimal native Linexin Center widget, the reference for widget authors.
//!
//! Install the built library as `/usr/share/linexin/widgets/<name>.so`; its
//! translations go to `/usr/share/linexin/widgets/localization/<lang>/rust_demo_dictionary.py`.
//!
//! It has both front ends: a GTK widget (`create`), and a LineXinBar page
//! (`lxb_create`/`lxb_page`/`lxb_destroy`) for when the Center runs as a
//! LineXinBar application. Both share nothing but the translations here; a
//! real widget shares its logic between them.

use std::cell::Cell;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::rc::Rc;

use gtk::glib::translate::IntoGlibPtr;
use gtk::prelude::*;
use linexin_plugin_api as api;

/// Widget id, which is also the translation domain.
const DOMAIN: &str = "rust_demo";

static WIDGET: api::WidgetDescriptorV1 = api::WidgetDescriptorV1::new(
    c"rust_demo",
    c"Rust Demo",
    c"applications-engineering-symbolic",
    Some(create),
)
.with_lxb_page(c"launch", lxb_create, lxb_page, lxb_destroy);
api::export_widget!(WIDGET);

fn tr(text: &str) -> String {
    api::tr(DOMAIN, text)
}

/// GTK calls back through C, where a panic must not unwind.
fn guarded(callback: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(callback)).is_err() {
        eprintln!("Rust Demo: callback panicked");
    }
}

unsafe extern "C" fn create(context: *const api::CreateContextV1) -> *mut c_void {
    let widget = catch_unwind(|| {
        // SAFETY: `context` is the pointer the host passed to `create`.
        unsafe { api::CreateContextV1::read(context) }?;
        // The library has its own gtk-rs state even though GTK is running.
        gtk::init().ok()?;
        // gtk-rs holds a sunk reference; hand it to the host (transfer full).
        let widget: *mut gtk::ffi::GtkWidget = build().upcast::<gtk::Widget>().into_glib_ptr();
        Some(widget.cast::<c_void>())
    });
    widget.ok().flatten().unwrap_or(ptr::null_mut())
}

fn build() -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.set_widget_name("linexin-demo-root");
    root.set_valign(gtk::Align::Center);
    root.set_margin_start(24);
    root.set_margin_end(24);

    let icon = gtk::Image::from_icon_name("applications-engineering-symbolic");
    icon.set_pixel_size(64);
    root.append(&icon);

    let title = gtk::Label::new(Some(&tr("Rust Demo")));
    title.add_css_class("title-1");
    root.append(&title);

    let description = gtk::Label::new(Some(&tr(
        "This widget is a native Rust library loaded by Linexin Center.",
    )));
    description.add_css_class("dim-label");
    description.set_wrap(true);
    description.set_justify(gtk::Justification::Center);
    root.append(&description);

    let count = gtk::Label::new(Some("0"));
    count.set_widget_name("linexin-demo-count");
    count.add_css_class("title-2");
    root.append(&count);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    buttons.set_halign(gtk::Align::Center);
    let increment = gtk::Button::with_label(&tr("Increment"));
    increment.set_widget_name("linexin-demo-increment");
    increment.add_css_class("suggested-action");
    let reset = gtk::Button::with_label(&tr("Reset"));
    reset.set_widget_name("linexin-demo-reset");
    buttons.append(&increment);
    buttons.append(&reset);
    root.append(&buttons);

    let clicks = Rc::new(Cell::new(0_u64));
    increment.connect_clicked({
        let (clicks, count) = (clicks.clone(), count.clone());
        move |_| {
            guarded(|| {
                clicks.set(clicks.get().saturating_add(1));
                count.set_label(&clicks.get().to_string());
            })
        }
    });
    reset.connect_clicked(move |_| {
        guarded(|| {
            clicks.set(0);
            count.set_label("0");
        })
    });
    root
}

/// The LineXinBar page's state: a counter and the flow that lays it out.
struct LxbState {
    flow: linexin_lxb::Flow,
    clicks: u64,
}

unsafe extern "C" fn lxb_create(context: *const api::LxbContextV1) -> *mut c_void {
    linexin_lxb::guard(ptr::null_mut(), || {
        // SAFETY: `context` is the pointer the host passed to `lxb_create`.
        if unsafe { api::LxbContextV1::read(context) }.is_none() {
            return ptr::null_mut();
        }
        let state = LxbState {
            flow: linexin_lxb::Flow::new(),
            clicks: 0,
        };
        Box::into_raw(Box::new(state)).cast()
    })
}

unsafe extern "C" fn lxb_page(state: *mut c_void, frame: *const api::LxbFrameV1) -> u32 {
    linexin_lxb::guard(api::LXB_PAGE_STAY, || {
        // SAFETY: the state from `lxb_create`, alive for the call.
        let Some(state) = (unsafe { state.cast::<LxbState>().as_mut() }) else {
            return api::LXB_PAGE_STAY;
        };
        // SAFETY: the host's frame, read for this call only.
        let Some(frame) = (unsafe { linexin_lxb::WidgetFrame::read(frame) }) else {
            return api::LXB_PAGE_STAY;
        };
        // Actions first (they move the light and press it), then the page.
        let input = state.flow.input(&frame.page, &frame.actions);
        let mut page = state.flow.begin(&frame.page, frame.rect, frame.focused);
        page.head("launch", &tr("Rust Demo"));
        page.text(&tr(
            "This widget is a native Rust library loaded by Linexin Center.",
        ));
        page.heading(&state.clicks.to_string());
        if page.button(&tr("Increment")) {
            state.clicks = state.clicks.saturating_add(1);
        }
        if page.button(&tr("Reset")) {
            state.clicks = 0;
        }
        page.finish();
        linexin_lxb::page_result(input.leave.is_some())
    })
}

unsafe extern "C" fn lxb_destroy(state: *mut c_void) {
    if !state.is_null() {
        // SAFETY: the state from `lxb_create`, released once.
        drop(unsafe { Box::from_raw(state.cast::<LxbState>()) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn descriptor_names_the_translation_domain() {
        assert_eq!(
            unsafe { CStr::from_ptr(WIDGET.widget_id) }.to_str(),
            Ok(DOMAIN)
        );
        assert_eq!(WIDGET.abi_version, api::ABI_VERSION);
        assert!(unsafe { linexin_widget_v1(ptr::null()) }.is_null());
        assert!(unsafe { create(ptr::null()) }.is_null());
    }
}
