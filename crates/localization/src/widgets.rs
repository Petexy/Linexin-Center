//! GTK helpers (GTK main thread only), translating through [`crate::global`].

use adw::prelude::*;

use crate::Localization;

/// Original `_translate_widget_recursive_static` + `_translate_single_widget_static`:
/// walk `widget` and all descendants (first_child/next_sibling) and replace,
/// via [`crate::Localization::translate`] (markup labels via
/// `translate_markup`), only when the result differs: Label text/markup,
/// Button label, Entry/SearchEntry placeholder, Adw.ActionRow title+subtitle
/// (+ Adw.ComboRow StringList items, preserving selection),
/// Adw.PreferencesGroup title+description, other Adw.PreferencesRow /
/// Adw.PreferencesPage title, Adw.StatusPage title+description, Gtk.Window title.
///
/// The original's `translate_window(window)` returned early for an empty
/// domain; callers implementing it must do the same.
pub fn translate_widget_tree(widget: &gtk::Widget, domain: &str) {
    let localization = crate::global();
    translate_single(localization, widget, domain);
    let mut child = widget.first_child();
    while let Some(current) = child {
        translate_widget_tree(&current, domain);
        child = current.next_sibling();
    }
}

/// Original `_translate_dialog`: translate heading/body of an
/// `adw::MessageDialog` or `adw::AlertDialog` (when `widget` is one), then
/// [`translate_widget_tree`]. Does nothing for an empty `domain`.
// Legacy widgets still create the deprecated (libadwaita 1.6) MessageDialog.
#[allow(deprecated)]
pub fn translate_dialog(widget: &gtk::Widget, domain: &str) {
    if domain.is_empty() {
        return;
    }
    let localization = crate::global();
    if let Some(dialog) = widget.downcast_ref::<adw::AlertDialog>() {
        if let Some(heading) = changed(localization, dialog.heading().as_deref(), domain) {
            dialog.set_heading(Some(&heading));
        }
        if let Some(body) = changed(localization, Some(&dialog.body()), domain) {
            dialog.set_body(&body);
        }
    } else if let Some(dialog) = widget.downcast_ref::<adw::MessageDialog>() {
        if let Some(heading) = changed(localization, dialog.heading().as_deref(), domain) {
            dialog.set_heading(Some(&heading));
        }
        if let Some(body) = changed(localization, Some(&dialog.body()), domain) {
            dialog.set_body(&body);
        }
    }
    translate_widget_tree(widget, domain);
}

/// The translation of a non-empty `text`, when it differs.
fn changed(localization: &Localization, text: Option<&str>, domain: &str) -> Option<String> {
    let text = text.filter(|text| !text.is_empty())?;
    let translated = localization.translate(text, domain);
    (translated != text).then_some(translated)
}

/// Same type checks in the same order as the original's `isinstance` chain.
fn translate_single(localization: &Localization, widget: &gtk::Widget, domain: &str) {
    let translated = |text: Option<&str>| changed(localization, text, domain);
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        if label.uses_markup() {
            let markup = label.label();
            if !markup.is_empty() {
                let translated_markup = localization.translate_markup(&markup, domain);
                if translated_markup != markup.as_str() {
                    label.set_markup(&translated_markup);
                }
            }
        } else if let Some(text) = translated(Some(&label.text())) {
            label.set_text(&text);
        }
    } else if let Some(button) = widget.downcast_ref::<gtk::Button>() {
        if let Some(label) = translated(button.label().as_deref()) {
            button.set_label(&label);
        }
    } else if let Some(entry) = widget.downcast_ref::<gtk::Entry>() {
        if let Some(placeholder) = translated(entry.placeholder_text().as_deref()) {
            entry.set_placeholder_text(Some(&placeholder));
        }
    } else if let Some(entry) = widget.downcast_ref::<gtk::SearchEntry>() {
        if let Some(placeholder) = translated(entry.placeholder_text().as_deref()) {
            entry.set_placeholder_text(Some(&placeholder));
        }
    } else if widget.is::<gtk::PasswordEntry>() {
        // The original called PasswordEntry.get_placeholder_text(), which
        // PyGObject does not provide; the AttributeError left it untranslated.
    } else if let Some(row) = widget.downcast_ref::<adw::ActionRow>() {
        if let Some(title) = translated(Some(&row.title())) {
            row.set_title(&title);
        }
        if let Some(subtitle) = translated(row.subtitle().as_deref()) {
            row.set_subtitle(&subtitle);
        }
        if let Some(combo) = row.downcast_ref::<adw::ComboRow>() {
            translate_combo_items(localization, combo, domain);
        }
    } else if let Some(group) = widget.downcast_ref::<adw::PreferencesGroup>() {
        if let Some(title) = translated(Some(&group.title())) {
            group.set_title(&title);
        }
        if let Some(description) = translated(group.description().as_deref()) {
            group.set_description(Some(&description));
        }
    } else if let Some(row) = widget.downcast_ref::<adw::PreferencesRow>() {
        if let Some(title) = translated(Some(&row.title())) {
            row.set_title(&title);
        }
    } else if let Some(page) = widget.downcast_ref::<adw::PreferencesPage>() {
        if let Some(title) = translated(Some(&page.title())) {
            page.set_title(&title);
        }
    } else if let Some(status) = widget.downcast_ref::<adw::StatusPage>() {
        if let Some(title) = translated(Some(&status.title())) {
            status.set_title(&title);
        }
        if let Some(description) = translated(status.description().as_deref()) {
            status.set_description(Some(&description));
        }
    } else if let Some(window) = widget.downcast_ref::<gtk::Window>()
        && let Some(title) = translated(window.title().as_deref())
    {
        window.set_title(Some(&title));
    }
}

/// Items of a `Gtk.StringList` model, re-spliced only when one changed.
fn translate_combo_items(localization: &Localization, combo: &adw::ComboRow, domain: &str) {
    let Some(model) = combo.model().and_downcast::<gtk::StringList>() else {
        return;
    };
    let count = model.n_items();
    let items: Vec<Option<gtk::glib::GString>> =
        (0..count).map(|position| model.string(position)).collect();
    let translated: Vec<String> = items
        .iter()
        .map(|item| match item.as_deref() {
            Some(text) if !text.is_empty() => localization.translate(text, domain),
            _ => String::new(),
        })
        .collect();
    let needs_update = items
        .iter()
        .zip(&translated)
        .any(|(item, translated)| item.as_deref().unwrap_or_default() != translated.as_str());
    if needs_update {
        let selected = combo.selected();
        let additions: Vec<&str> = translated.iter().map(String::as_str).collect();
        model.splice(0, count, &additions);
        combo.set_selected(selected);
    }
}
