//! The original `_apply_styling` providers.

use gtk::prelude::*;

const STYLE: &str = include_str!("style.css");
const KINEXIN_STYLE: &str = include_str!("kinexin.css");

pub(super) fn apply(window: &gtk::ApplicationWindow, kinexin: bool) {
    let display = WidgetExt::display(window);
    let provider = gtk::CssProvider::new();
    provider.load_from_string(STYLE);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    if kinexin {
        // Above USER priority so it overrides ~/.config/gtk-4.0/gtk.css.
        let kinexin_provider = gtk::CssProvider::new();
        kinexin_provider.load_from_string(KINEXIN_STYLE);
        gtk::style_context_add_provider_for_display(
            &display,
            &kinexin_provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER + 1,
        );
    }
}
