//! Content area: header bar, widget stack, placeholder/error pages and the
//! single-widget header controls.

use super::Inner;
use super::entries::{Entry, Instance, LoadState, Source};
use super::sidebar::{create_invisible_separator, parse_button_layout};
use adw::prelude::*;
use linexin_localization::{global, tr};
use std::rc::Rc;

/// Original `_create_persistent_widgets` stack with its initial page.
pub(super) fn create_stack(single_widget: bool) -> gtk::Stack {
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::SlideUpDown);
    stack.set_transition_duration(300);
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    if single_widget {
        let loading_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
        loading_box.set_valign(gtk::Align::Center);
        loading_box.set_halign(gtk::Align::Center);
        let spinner = gtk::Spinner::new();
        spinner.set_size_request(32, 32);
        spinner.start();
        loading_box.append(&spinner);
        let label = gtk::Label::new(Some(&tr("Loading...")));
        label.add_css_class("dim-label");
        loading_box.append(&label);
        loading_box.set_vexpand(true);
        loading_box.set_hexpand(true);
        stack.add_named(&loading_box, Some("placeholder"));
    } else {
        let placeholder = gtk::Label::new(Some(&tr("Select a widget from the sidebar")));
        placeholder.set_vexpand(true);
        placeholder.set_hexpand(true);
        stack.add_named(&placeholder, Some("placeholder"));
    }
    stack.set_visible_child_name("placeholder");
    stack
}

/// Original `_build_content_area` + `_create_content_header`.
pub(super) fn build_content_area(
    content_display: &adw::Bin,
    kinexin: bool,
    single_widget: bool,
    button_layout: &str,
) -> (adw::NavigationPage, gtk::HeaderBar) {
    let page = adw::NavigationPage::builder()
        .title(tr("Widget Content"))
        .build();
    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    if kinexin {
        container.add_css_class("kinexin-content");
    }
    let header = gtk::HeaderBar::new();
    header.add_css_class("flat");
    apply_button_layout(&header, button_layout, kinexin, single_widget);
    // The original keeps the content header's title empty.
    header.set_title_widget(Some(&gtk::Label::new(Some(""))));
    container.append(&header);
    container.append(&create_invisible_separator());
    container.append(content_display);
    page.set_child(Some(&container));
    (page, header)
}

/// Window controls of the content header: none on Plasma/Hyprland (server-side
/// decorations), the whole layout in single-widget mode, otherwise the part
/// after `:`.
pub(super) fn apply_button_layout(
    header: &gtk::HeaderBar,
    layout: &str,
    kinexin: bool,
    single_widget: bool,
) {
    if kinexin {
        header.set_show_title_buttons(false);
    } else if single_widget {
        header.set_show_title_buttons(true);
        header.set_decoration_layout(Some(layout));
    } else {
        let (_, right) = parse_button_layout(layout);
        if right.is_empty() {
            header.set_show_title_buttons(false);
        } else {
            header.set_show_title_buttons(true);
            header.set_decoration_layout(Some(&format!(":{right}")));
        }
    }
}

/// Original `_create_error_widget`.
pub(super) fn create_error_widget(widget_name: &str, message: &str) -> gtk::Label {
    let text = format!("{} '{widget_name}': {message}", tr("Error loading"));
    let label = gtk::Label::new(Some(&text));
    label.set_vexpand(true);
    label.set_hexpand(true);
    label.add_css_class("dim-label");
    label
}

impl Inner {
    /// Original `_add_widget_to_stack`.
    pub(super) fn add_widget_to_stack(&self, entry: &Entry, page_name: &str) {
        let stack = &self.ui.widget_stack;
        if stack.child_by_name(page_name).is_some() {
            return;
        }
        let page: gtk::Widget = match &*entry.state.borrow() {
            LoadState::Loaded(instance) if instance.widget().parent().is_none() => {
                instance.widget().clone()
            }
            LoadState::Failed(message) => {
                create_error_widget(&entry.widget_name(), message).upcast()
            }
            _ => create_error_widget(&entry.widget_name(), &tr("Widget cannot be displayed"))
                .upcast(),
        };
        stack.add_named(&page, Some(page_name));
    }

    /// A spinner page shown while a deferred Python widget is imported.
    pub(super) fn show_loading_spinner(&self, entry: &Rc<Entry>) {
        let loading_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
        loading_box.set_valign(gtk::Align::Center);
        loading_box.set_halign(gtk::Align::Center);
        let spinner = gtk::Spinner::new();
        spinner.set_size_request(32, 32);
        spinner.start();
        loading_box.append(&spinner);
        loading_box.set_vexpand(true);
        loading_box.set_hexpand(true);
        let page_name = format!("_loading_{}", entry.widget_name());
        let stack = &self.ui.widget_stack;
        if let Some(old) = stack.child_by_name(&page_name) {
            stack.remove(&old);
        }
        stack.add_named(&loading_box, Some(&page_name));
        stack.set_visible_child_name(&page_name);
        self.loading_spinner_page
            .replace(Some((page_name, Rc::downgrade(entry))));
    }

    /// Remove the spinner page if it belongs to `entry`.
    pub(super) fn remove_loading_spinner(&self, entry: &Rc<Entry>) {
        let belongs = self
            .loading_spinner_page
            .borrow()
            .as_ref()
            .is_some_and(|(_, owner)| owner.as_ptr() == Rc::as_ptr(entry));
        if !belongs {
            return;
        }
        if let Some((page_name, _)) = self.loading_spinner_page.take() {
            let stack = &self.ui.widget_stack;
            if let Some(child) = stack.child_by_name(&page_name) {
                stack.remove(&child);
            }
        }
    }

    /// Original error label of `_select_target_widget`.
    pub(super) fn show_single_widget_error(&self) {
        let target = self.options.target_widget.as_deref().unwrap_or_default();
        let text = global().translate(
            &format!("Widget '{target}' not found or failed to load"),
            linexin_localization::HOST_DOMAIN,
        );
        let label = gtk::Label::new(Some(&text));
        label.set_vexpand(true);
        label.set_hexpand(true);
        label.add_css_class("dim-label");
        let stack = &self.ui.widget_stack;
        if let Some(old) = stack.child_by_name("error") {
            stack.remove(&old);
        }
        stack.add_named(&label, Some("error"));
        stack.set_visible_child_name("error");
    }

    /// Original `update_widget_header`: single-widget header controls go on
    /// the side opposite to the window buttons.
    pub(super) fn update_widget_header(&self) {
        if !self.options.hide_sidebar() {
            return;
        }
        let header = &self.ui.content_header;
        if let Some(old) = self.widget_header_widget.take() {
            header.remove(&old);
        }
        let Some(entry) = self.active() else {
            return;
        };
        let header_widget = match &*entry.state.borrow() {
            LoadState::Loaded(Instance::Python(loaded)) => self
                .services
                .python
                .as_ref()
                .and_then(|python| python.header_bar_widget(loaded)),
            LoadState::Loaded(Instance::Native(widget)) => match &entry.source {
                Source::Native(native) => native.header_bar_widget(widget),
                _ => None,
            },
            _ => None,
        };
        if let Some(header_widget) = header_widget {
            let layout = self.button_layout.borrow().clone();
            if parse_button_layout(&layout).0.is_empty() {
                header.pack_start(&header_widget);
            } else {
                header.pack_end(&header_widget);
            }
            self.widget_header_widget.replace(Some(header_widget));
        }
    }
}
