//! Sidebar context menu: create or remove a desktop shortcut for a widget
//! (`/usr/share/applications/<file stem>.desktop`, written with sudo).

use super::Inner;
use super::entries::Entry;
use crate::shortcut::{exists, run_privileged};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use linexin_localization::{global, tr, widgets};
use linexin_services::sudo;
use std::cell::RefCell;
use std::rc::Rc;

/// Continuation run once the password has been accepted.
type AfterAuthentication = Box<dyn FnOnce(&Rc<Inner>)>;

enum ShortcutAction {
    Create,
    Remove,
}

impl Inner {
    /// Original `_on_sidebar_right_click`.
    pub(super) fn on_sidebar_right_click(&self, x: f64, y: f64) {
        let Some(sidebar) = self.sidebar() else {
            return;
        };
        let Some(entry) = self
            .ui
            .widget_list
            .row_at_y(y as i32)
            .and_then(|row| self.entry_for_row(&row))
        else {
            return;
        };
        self.context_menu_entry.replace(Some(Rc::downgrade(&entry)));
        let menu = gio::Menu::new();
        if exists(&entry.file_name) {
            menu.append(Some(&tr("Remove shortcut")), Some("win.remove_shortcut"));
        } else {
            menu.append(Some(&tr("Create a shortcut")), Some("win.create_shortcut"));
        }
        let popover = &sidebar.popover;
        popover.set_menu_model(Some(&menu));
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.set_position(gtk::PositionType::Bottom);
        popover.popup();
    }

    pub(super) fn on_create_shortcut(self: &Rc<Self>) {
        self.start_shortcut_action(
            ShortcutAction::Create,
            "Please enter your password to create the shortcut.",
        );
    }

    pub(super) fn on_remove_shortcut(self: &Rc<Self>) {
        self.start_shortcut_action(
            ShortcutAction::Remove,
            "Please enter your password to remove the shortcut.",
        );
    }

    fn start_shortcut_action(self: &Rc<Self>, action: ShortcutAction, prompt: &str) {
        let Some(entry) = self.find_entry(&self.context_menu_entry.borrow()) else {
            return;
        };
        if sudo::manager().has_password() {
            self.perform_shortcut_action(&entry, action);
        } else {
            let weak_entry = Rc::downgrade(&entry);
            self.prompt_for_password(
                &tr(prompt),
                Box::new(move |this| {
                    if let Some(entry) = weak_entry.upgrade() {
                        this.perform_shortcut_action(&entry, action);
                    }
                }),
            );
        }
    }

    /// Original `_prompt_for_password_dialog`; the password is checked with
    /// `sudo -S -v` on a worker thread.
    fn prompt_for_password(self: &Rc<Self>, message: &str, on_success: AfterAuthentication) {
        let Some(window) = self.window() else {
            return;
        };
        #[allow(deprecated)] // Adw.MessageDialog, as in the original.
        let dialog = adw::MessageDialog::new(
            Some(&window),
            Some(&tr("Authentication Required")),
            Some(message),
        );
        dialog.add_response("cancel", &tr("Cancel"));
        dialog.add_response("authenticate", &tr("Authenticate"));
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let password_entry = gtk::PasswordEntry::new();
        content.append(&password_entry);
        dialog.set_extra_child(Some(&content));

        let weak = Rc::downgrade(self);
        let on_success = RefCell::new(Some(on_success));
        dialog.connect_response(None, move |_, response| {
            if response != "authenticate" {
                return;
            }
            let (Some(this), Some(on_success)) = (weak.upgrade(), on_success.take()) else {
                return;
            };
            let password = password_entry.text().to_string();
            glib::MainContext::default().spawn_local(async move {
                let valid = gio::spawn_blocking(move || {
                    let manager = sudo::manager();
                    let valid = manager.validate_password(&password);
                    if valid {
                        manager.set_password(&password);
                    }
                    valid
                })
                .await
                .unwrap_or(false);
                if valid {
                    on_success(&this);
                } else {
                    this.show_message(&tr("Authentication Failed"), &tr("Incorrect password."));
                }
            });
        });
        dialog.set_default_response(Some("authenticate"));
        self.translate_dialog(dialog.upcast_ref());
        dialog.present();
    }

    /// Original `_perform_shortcut_creation` / `_perform_shortcut_removal`.
    /// The stored password is forgotten afterwards in every case.
    fn perform_shortcut_action(self: &Rc<Self>, entry: &Entry, action: ShortcutAction) {
        let display_name = global().get_text(&entry.widget_name(), &entry.widget_id());
        let file_name = entry.file_name.clone();
        let icon = entry.meta.borrow().icon.clone();
        let create = matches!(action, ShortcutAction::Create);
        let name = display_name.clone();
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || {
                let result = crate::shortcut::change(
                    &file_name,
                    &name,
                    icon.as_deref(),
                    create,
                    run_privileged,
                );
                sudo::manager().forget_password();
                result
            })
            .await
            .unwrap_or_else(|_| Err("the operation failed unexpectedly".into()));
            let Some(this) = weak.upgrade() else {
                return;
            };
            match result {
                Ok(Some((heading, body))) => {
                    this.show_message(&tr(heading), &tr(body).replacen("{}", &display_name, 1));
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("Error updating shortcut: {error}");
                    this.show_message(&tr("Error"), &error);
                }
            }
        });
    }

    fn show_message(&self, heading: &str, body: &str) {
        let Some(window) = self.window() else {
            return;
        };
        #[allow(deprecated)] // Adw.MessageDialog, as in the original.
        let dialog = adw::MessageDialog::new(Some(&window), Some(heading), Some(body));
        dialog.add_response("ok", &tr("OK"));
        self.translate_dialog(dialog.upcast_ref());
        dialog.present();
    }

    /// Original `_translate_dialog(dialog)` without an explicit domain: the
    /// active widget's dictionary.
    fn translate_dialog(&self, dialog: &gtk::Widget) {
        if let Some(entry) = self.active() {
            widgets::translate_dialog(dialog, &entry.widget_id());
        }
    }
}
