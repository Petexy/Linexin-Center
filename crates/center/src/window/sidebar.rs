//! Sidebar structure (original `_build_sidebar`) and window button layout.

use super::Inner;
use adw::prelude::*;
use gtk::{gio, glib};
use linexin_localization::tr;
use std::rc::Weak;

const WM_PREFERENCES: &str = "org.gnome.desktop.wm.preferences";

pub(super) struct Sidebar {
    pub page: adw::NavigationPage,
    pub container: gtk::Box,
    pub handle: gtk::WindowHandle,
    pub header: gtk::HeaderBar,
    pub title: gtk::Label,
    pub list_container: gtk::Box,
    pub compact_strip: gtk::Box,
    pub compact_scroll: gtk::ScrolledWindow,
    pub bottom_box: gtk::Box,
    pub edit_button: gtk::Button,
    pub compact_button: gtk::Button,
    pub compact_button_css: gtk::CssProvider,
    pub popover: gtk::PopoverMenu,
}

impl Sidebar {
    pub fn build(
        weak: &Weak<Inner>,
        window: &gtk::ApplicationWindow,
        widget_list: &gtk::ListBox,
        kinexin: bool,
        button_layout: &str,
    ) -> Self {
        let page = adw::NavigationPage::builder().title(tr("Widgets")).build();
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.set_hexpand(true);
        if kinexin {
            container.add_css_class("kinexin-sidebar");
        }

        let header = gtk::HeaderBar::new();
        header.add_css_class("flat");
        let title = gtk::Label::new(Some(&tr("Linexin Center")));
        title.add_css_class("heading");
        header.set_title_widget(Some(&title));
        container.append(&header);
        container.append(&create_invisible_separator());

        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scrolled.set_hexpand(false);
        scrolled.set_vexpand(true);
        scrolled.set_child(Some(widget_list));
        let list_container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        list_container.set_margin_top(6);
        list_container.set_margin_bottom(6);
        list_container.set_margin_start(6);
        list_container.set_margin_end(6);
        list_container.append(&scrolled);
        container.append(&list_container);

        // Compact mode icon strip (initially hidden).
        let compact_strip = gtk::Box::new(gtk::Orientation::Vertical, 0);
        compact_strip.set_halign(gtk::Align::Center);
        compact_strip.set_valign(gtk::Align::Start);
        compact_strip.set_vexpand(true);
        compact_strip.set_visible(false);
        compact_strip.set_margin_top(6);
        compact_strip.set_margin_bottom(6);
        let compact_scroll = gtk::ScrolledWindow::new();
        compact_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        compact_scroll.set_vexpand(true);
        compact_scroll.set_child(Some(&compact_strip));
        compact_scroll.set_visible(false);
        container.append(&compact_scroll);

        // Bottom bar: edit + compact toggle.
        let bottom_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bottom_box.set_halign(gtk::Align::Center);
        bottom_box.set_margin_bottom(16);
        bottom_box.set_margin_top(4);

        let edit_button = gtk::Button::from_icon_name("document-edit-symbolic");
        edit_button.add_css_class("flat");
        edit_button.add_css_class("edit-toggle-btn");
        edit_button.set_tooltip_text(Some(&tr("Edit widgets")));
        let edit_weak = weak.clone();
        edit_button.connect_clicked(move |_| {
            if let Some(this) = edit_weak.upgrade() {
                this.on_edit_toggle_clicked();
            }
        });
        bottom_box.append(&edit_button);

        let compact_button = gtk::Button::from_icon_name("sidebar-show-symbolic");
        compact_button.add_css_class("flat");
        compact_button.add_css_class("compact-toggle-btn");
        compact_button.set_tooltip_text(Some(&tr("Toggle compact mode")));
        let compact_weak = weak.clone();
        compact_button.connect_clicked(move |_| {
            if let Some(this) = compact_weak.upgrade() {
                this.on_compact_toggle_clicked();
            }
        });
        bottom_box.append(&compact_button);
        // The button's own provider carries its rotation (180° in normal mode).
        let compact_button_css = gtk::CssProvider::new();
        compact_button_css.load_from_string(
            ".compact-toggle-btn { transition: none; transform: rotate(180deg); }",
        );
        #[allow(deprecated)] // Per-widget provider, exactly as the original.
        compact_button.style_context().add_provider(
            &compact_button_css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
        container.append(&bottom_box);

        let popover = setup_context_menu(weak, window, widget_list);

        let handle = gtk::WindowHandle::new();
        handle.set_child(Some(&container));
        page.set_child(Some(&handle));

        let sidebar = Self {
            page,
            container,
            handle,
            header,
            title,
            list_container,
            compact_strip,
            compact_scroll,
            bottom_box,
            edit_button,
            compact_button,
            compact_button_css,
            popover,
        };
        sidebar.apply_button_layout(button_layout, kinexin);
        sidebar
    }

    /// Original `_create_sidebar_header` window-control setup: left buttons
    /// live in the sidebar header.
    pub fn apply_button_layout(&self, layout: &str, kinexin: bool) {
        let (header, title) = (&self.header, &self.title);
        if kinexin {
            header.set_show_title_buttons(false);
            title.set_visible(false);
            return;
        }
        let (left, _) = parse_button_layout(layout);
        if left.is_empty() {
            header.set_show_title_buttons(false);
            title.set_margin_top(10);
            title.set_margin_start(0);
        } else {
            header.set_show_title_buttons(true);
            header.set_decoration_layout(Some(&format!("{left}:")));
            title.set_margin_top(0);
            title.set_margin_start(30);
        }
    }
}

/// Original `_setup_sidebar_context_menu`.
fn setup_context_menu(
    weak: &Weak<Inner>,
    window: &gtk::ApplicationWindow,
    widget_list: &gtk::ListBox,
) -> gtk::PopoverMenu {
    let menu = gio::Menu::new();
    menu.append_item(&gio::MenuItem::new(
        Some(&tr("Create a shortcut")),
        Some("win.create_shortcut"),
    ));
    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(widget_list);
    popover.set_has_arrow(false);

    let create = gio::SimpleAction::new("create_shortcut", None);
    let create_weak = weak.clone();
    create.connect_activate(move |_, _| {
        if let Some(this) = create_weak.upgrade() {
            this.on_create_shortcut();
        }
    });
    window.add_action(&create);
    let remove = gio::SimpleAction::new("remove_shortcut", None);
    let remove_weak = weak.clone();
    remove.connect_activate(move |_, _| {
        if let Some(this) = remove_weak.upgrade() {
            this.on_remove_shortcut();
        }
    });
    window.add_action(&remove);

    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    let gesture_weak = weak.clone();
    gesture.connect_pressed(move |_, _, x, y| {
        if let Some(this) = gesture_weak.upgrade() {
            this.on_sidebar_right_click(x, y);
        }
    });
    widget_list.add_controller(gesture);
    popover
}

/// Original `_create_invisible_separator`.
pub(super) fn create_invisible_separator() -> gtk::Separator {
    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    separator.add_css_class("hidden-separator");
    separator
}

/// GNOME's window-manager preferences, when the schema is installed.
pub(super) fn wm_preferences() -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(WM_PREFERENCES, true)?;
    schema
        .has_key("button-layout")
        .then(|| gio::Settings::new(WM_PREFERENCES))
}

/// Current `button-layout`; GTK's own decoration layout without the schema.
pub(super) fn button_layout(settings: Option<&gio::Settings>) -> String {
    match settings {
        Some(settings) => settings.string("button-layout").into(),
        None => gtk::Settings::default()
            .and_then(|settings| settings.gtk_decoration_layout())
            .map(glib::GString::into)
            .unwrap_or_else(|| "menu:close".to_owned()),
    }
}

/// Original `_parse_button_layout`: `(left, right)` around the first `:`.
pub(super) fn parse_button_layout(layout: &str) -> (&str, &str) {
    layout.split_once(':').unwrap_or((layout, ""))
}

#[cfg(test)]
mod tests {
    use super::parse_button_layout;

    #[test]
    fn button_layout_parsing() {
        assert_eq!(parse_button_layout("appmenu:close"), ("appmenu", "close"));
        assert_eq!(
            parse_button_layout("close,minimize,maximize:icon"),
            ("close,minimize,maximize", "icon")
        );
        assert_eq!(
            parse_button_layout(":minimize,close"),
            ("", "minimize,close")
        );
        assert_eq!(parse_button_layout("close"), ("close", ""));
    }
}
