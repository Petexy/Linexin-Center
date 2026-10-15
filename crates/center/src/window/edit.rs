//! Edit mode: reorder (buttons and drag-and-drop) and hide widgets, with the
//! original staggered animations.

use super::Inner;
use super::entries::Entry;
use crate::config;
use adw::prelude::*;
use gtk::{gdk, glib};
use linexin_localization::tr;
use std::rc::Rc;

impl Inner {
    /// Original `_on_edit_toggle_clicked`.
    pub(super) fn on_edit_toggle_clicked(self: &Rc<Self>) {
        if self.sidebar().is_none() {
            return;
        }
        if self.edit_mode.get() {
            self.exit_edit_mode();
        } else {
            self.enter_edit_mode();
        }
    }

    /// Original `_enter_edit_mode`.
    fn enter_edit_mode(self: &Rc<Self>) {
        let Some(sidebar) = self.sidebar() else {
            return;
        };
        self.edit_was_auto_compact
            .set(self.auto_compact_active.get());
        if self.compact_mode.get() {
            // Let the sidebar extend in narrow windows.
            if self.auto_compact_active.get() {
                self.apply_edit_mode_content_shrink();
            }
            self.set_compact_mode(false, true);
        }
        self.edit_mode.set(true);
        sidebar.compact_button.set_sensitive(false);

        // Without the window handle, dragging rows reorders them instead of
        // moving the window.
        sidebar.handle.set_child(None::<&gtk::Widget>);
        sidebar.page.set_child(Some(&sidebar.container));

        sidebar.edit_button.add_css_class("edit-btn-spinning");
        self.after(300, |this| {
            if let Some(sidebar) = this.sidebar() {
                let button = &sidebar.edit_button;
                button.remove_css_class("edit-btn-spinning");
                button.set_icon_name("object-select-symbolic");
                button.set_tooltip_text(Some(&tr("Done")));
                button.add_css_class("suggested-action");
                button.add_css_class("edit-btn-active");
            }
        });

        let entries = self.entries.borrow().clone();
        for (index, entry) in entries.iter().enumerate() {
            let row = &entry.row;
            row.set_visible(true);
            if entry.hidden.get() {
                row.add_css_class("edit-hidden-row");
            }
            row.add_css_class("edit-row-enter-prep");
            self.add_edit_controls_to_row(entry);
            self.attach_dnd_controllers(entry);
            let row = row.clone();
            self.after(60 + index as u64 * 40, move |this| {
                row.remove_css_class("edit-row-enter-prep");
                row.add_css_class("edit-row-enter");
                this.after(400, move |_| row.remove_css_class("edit-row-enter"));
            });
        }
    }

    /// Original `_exit_edit_mode`.
    fn exit_edit_mode(self: &Rc<Self>) {
        let Some(sidebar) = self.sidebar() else {
            return;
        };
        self.edit_mode.set(false);
        sidebar.compact_button.set_sensitive(true);
        self.remove_edit_mode_content_shrink();
        if self.edit_was_auto_compact.replace(false) {
            let width = self.window().map_or(0, |window| window.default_size().0);
            if width > 0 && width < 1000 {
                self.auto_compact_active.set(true);
                self.after(400, |this| this.set_compact_mode(true, true));
            }
        }

        sidebar.page.set_child(None::<&gtk::Widget>);
        sidebar.handle.set_child(Some(&sidebar.container));
        sidebar.page.set_child(Some(&sidebar.handle));

        let button = &sidebar.edit_button;
        button.remove_css_class("edit-btn-active");
        button.remove_css_class("suggested-action");
        button.add_css_class("edit-btn-spinning");
        self.after(300, |this| {
            if let Some(sidebar) = this.sidebar() {
                let button = &sidebar.edit_button;
                button.remove_css_class("edit-btn-spinning");
                button.set_icon_name("document-edit-symbolic");
                button.set_tooltip_text(Some(&tr("Edit widgets")));
            }
        });

        let entries = self.entries.borrow().clone();
        for (index, entry) in entries.iter().enumerate() {
            entry.row.remove_css_class("edit-drop-above");
            entry.row.remove_css_class("edit-drop-below");
            let row = entry.row.clone();
            self.after(index as u64 * 30, move |_| {
                row.add_css_class("edit-row-exit")
            });
        }
        self.after(
            entries.len() as u64 * 30 + 350,
            Inner::finish_exit_edit_mode,
        );
    }

    /// Original `_finish_exit_edit_mode`.
    fn finish_exit_edit_mode(self: &Rc<Self>) {
        let entries = self.entries.borrow().clone();
        for entry in &entries {
            let row = &entry.row;
            for class in [
                "edit-hidden-row",
                "edit-row-exit",
                "edit-row-enter",
                "edit-row-enter-prep",
            ] {
                row.remove_css_class(class);
            }
            if let Some(controls) = entry.edit_controls.take() {
                row.remove(&controls);
            }
            for controller in entry.dnd_controllers.take() {
                row.remove_controller(&controller);
            }
            row.set_visible(!entry.hidden.get());
            entry.compact_button.set_visible(!entry.hidden.get());
        }
        let selected_hidden = self
            .ui
            .widget_list
            .selected_row()
            .and_then(|row| self.entry_for_row(&row))
            .is_some_and(|entry| entry.hidden.get());
        if selected_hidden {
            self.auto_select_first_visible_widget();
        }
        self.save_widget_config();
    }

    /// Original `_add_edit_controls_to_row`.
    fn add_edit_controls_to_row(self: &Rc<Self>, entry: &Rc<Entry>) {
        let suffix_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        suffix_box.add_css_class("edit-controls-box");

        let visibility = gtk::Button::new();
        visibility.add_css_class("flat");
        visibility.add_css_class("edit-vis-btn");
        set_visibility_icon(&visibility, entry.hidden.get());
        let (weak_self, weak_entry) = (Rc::downgrade(self), Rc::downgrade(entry));
        visibility.connect_clicked(move |button| {
            if let (Some(this), Some(entry)) = (weak_self.upgrade(), weak_entry.upgrade()) {
                this.toggle_widget_visibility(button, &entry);
            }
        });
        suffix_box.append(&visibility);

        for (icon, tooltip, step) in [
            ("go-up-symbolic", "Move up", -1),
            ("go-down-symbolic", "Move down", 1),
        ] {
            let button = gtk::Button::new();
            button.add_css_class("flat");
            button.add_css_class("edit-move-btn");
            button.set_icon_name(icon);
            button.set_tooltip_text(Some(&tr(tooltip)));
            let (weak_self, weak_entry) = (Rc::downgrade(self), Rc::downgrade(entry));
            button.connect_clicked(move |_| {
                if let (Some(this), Some(entry)) = (weak_self.upgrade(), weak_entry.upgrade()) {
                    this.move_widget_by(&entry, step);
                }
            });
            suffix_box.append(&button);
        }

        entry.row.add_suffix(&suffix_box);
        if let Some(old) = entry.edit_controls.replace(Some(suffix_box)) {
            entry.row.remove(&old);
        }
    }

    /// Original `_on_toggle_widget_visibility`.
    fn toggle_widget_visibility(&self, button: &gtk::Button, entry: &Entry) {
        let hidden = !entry.hidden.get();
        entry.hidden.set(hidden);
        let mut hidden_widgets = self.hidden_widgets.borrow_mut();
        if hidden {
            if !hidden_widgets.contains(&entry.file_name) {
                hidden_widgets.push(entry.file_name.clone());
            }
            entry.row.add_css_class("edit-hidden-row");
        } else {
            let names: Vec<String> = config::hidden_names(&entry.file_name).collect();
            hidden_widgets.retain(|name| !names.contains(name));
            entry.row.remove_css_class("edit-hidden-row");
        }
        set_visibility_icon(button, hidden);
    }

    /// Original `_on_move_widget_up` / `_on_move_widget_down`.
    fn move_widget_by(&self, entry: &Entry, step: isize) {
        let Some(index) = self.index_of(entry) else {
            return;
        };
        let Some(target) = index.checked_add_signed(step) else {
            return;
        };
        if target < self.entries.borrow().len() {
            self.entries.borrow_mut().swap(index, target);
            self.rebuild_order();
        }
    }

    /// Original `_move_widget_to`.
    fn move_widget_to(&self, from: usize, to: usize) {
        let len = self.entries.borrow().len();
        if from == to || from >= len || to >= len {
            return;
        }
        {
            let mut entries = self.entries.borrow_mut();
            let entry = entries.remove(from);
            entries.insert(to, entry);
        }
        self.rebuild_order();
    }

    /// Original `_rebuild_listbox_order` + `_rebuild_compact_strip_order`.
    fn rebuild_order(&self) {
        let entries = self.entries.borrow().clone();
        let list = &self.ui.widget_list;
        for entry in &entries {
            list.remove(&entry.row);
        }
        for entry in &entries {
            list.append(&entry.row);
        }
        if let Some(sidebar) = self.sidebar() {
            for entry in &entries {
                sidebar.compact_strip.remove(&entry.compact_button);
            }
            for entry in &entries {
                sidebar.compact_strip.append(&entry.compact_button);
            }
        }
    }

    /// Original `_attach_dnd_controllers`.
    fn attach_dnd_controllers(self: &Rc<Self>, entry: &Rc<Entry>) {
        let row = &entry.row;
        let drag_source = gtk::DragSource::new();
        drag_source.set_actions(gdk::DragAction::MOVE);
        let (weak_self, weak_entry) = (Rc::downgrade(self), Rc::downgrade(entry));
        drag_source.connect_prepare(move |_, _, _| {
            let (this, entry) = (weak_self.upgrade()?, weak_entry.upgrade()?);
            let index = i32::try_from(this.index_of(&entry)?).ok()?;
            Some(gdk::ContentProvider::for_value(&index.to_value()))
        });
        let weak_entry = Rc::downgrade(entry);
        drag_source.connect_drag_begin(move |_, drag| {
            if let Some(entry) = weak_entry.upgrade() {
                on_drag_begin(&entry, drag);
            }
        });
        let weak_entry = Rc::downgrade(entry);
        drag_source.connect_drag_end(move |_, _, _| {
            if let Some(entry) = weak_entry.upgrade() {
                entry.row.remove_css_class("edit-dragging");
            }
        });
        row.add_controller(drag_source.clone());

        let drop_target = gtk::DropTarget::new(glib::Type::I32, gdk::DragAction::MOVE);
        let row_weak = row.downgrade();
        drop_target.connect_motion(move |_, _, y| {
            if let Some(row) = row_weak.upgrade() {
                let above = y < f64::from(row.height()) / 2.0;
                let (add, remove) = if above {
                    ("edit-drop-above", "edit-drop-below")
                } else {
                    ("edit-drop-below", "edit-drop-above")
                };
                row.add_css_class(add);
                row.remove_css_class(remove);
            }
            gdk::DragAction::MOVE
        });
        let row_weak = row.downgrade();
        drop_target.connect_leave(move |_| {
            if let Some(row) = row_weak.upgrade() {
                clear_drop_indicators(&row);
            }
        });
        let (weak_self, weak_entry) = (Rc::downgrade(self), Rc::downgrade(entry));
        drop_target.connect_drop(move |_, value, _, y| {
            match (weak_self.upgrade(), weak_entry.upgrade()) {
                (Some(this), Some(entry)) => this.on_drop(&entry, value, y),
                _ => false,
            }
        });
        row.add_controller(drop_target.clone());
        entry
            .dnd_controllers
            .replace(vec![drag_source.upcast(), drop_target.upcast()]);
    }

    /// Original `_on_drop_drop`.
    fn on_drop(&self, entry: &Entry, value: &glib::Value, y: f64) -> bool {
        let row = &entry.row;
        let source = value
            .get::<i32>()
            .ok()
            .and_then(|v| usize::try_from(v).ok());
        let (Some(source), Some(mut target)) = (source, self.index_of(entry)) else {
            clear_drop_indicators(row);
            return false;
        };
        if source == target {
            clear_drop_indicators(row);
            return false;
        }
        let last = self.entries.borrow().len().saturating_sub(1);
        let below = y >= f64::from(row.height()) / 2.0;
        if below && target < source {
            target = (target + 1).min(last);
        } else if !below && target > source {
            target = target.saturating_sub(1);
        }
        clear_drop_indicators(row);
        self.move_widget_to(source, target);
        true
    }

    /// Original `_save_widget_config`.
    fn save_widget_config(&self) {
        let order: Vec<String> = self
            .entries
            .borrow()
            .iter()
            .map(|entry| entry.file_name.clone())
            .collect();
        let hidden = self.hidden_widgets.borrow().clone();
        self.services.config.update(|config| {
            config::set_names(config, config::WIDGET_ORDER, &order);
            config::set_names(config, config::HIDDEN_WIDGETS, &hidden);
        });
    }

    /// Original `_apply_edit_mode_content_shrink`.
    fn apply_edit_mode_content_shrink(&self) {
        let Some(window) = self.window() else {
            return;
        };
        let provider = self
            .edit_shrink_provider
            .borrow_mut()
            .get_or_insert_with(gtk::CssProvider::new)
            .clone();
        provider.load_from_string(
            ".buttons_all { min-width: 0; }\n\
             .compact-icon-btn { min-width: 0; }\n\
             .edit-move-btn, .edit-vis-btn { min-width: 0; }",
        );
        gtk::style_context_add_provider_for_display(
            &WidgetExt::display(&window),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
        );
        self.ui.content_display.set_size_request(0, -1);
    }

    /// Original `_remove_edit_mode_content_shrink`.
    fn remove_edit_mode_content_shrink(&self) {
        if let (Some(provider), Some(window)) =
            (self.edit_shrink_provider.borrow().as_ref(), self.window())
        {
            gtk::style_context_remove_provider_for_display(&WidgetExt::display(&window), provider);
        }
        self.ui.content_display.set_size_request(-1, -1);
    }
}

fn set_visibility_icon(button: &gtk::Button, hidden: bool) {
    if hidden {
        button.set_icon_name("view-reveal-symbolic");
        button.set_tooltip_text(Some(&tr("Show widget")));
    } else {
        button.set_icon_name("view-conceal-symbolic");
        button.set_tooltip_text(Some(&tr("Hide widget")));
    }
}

fn clear_drop_indicators(row: &impl IsA<gtk::Widget>) {
    row.remove_css_class("edit-drop-above");
    row.remove_css_class("edit-drop-below");
}

/// Original `_on_drag_begin`: the row's name as the drag icon.
fn on_drag_begin(entry: &Entry, drag: &gdk::Drag) {
    let row = &entry.row;
    row.add_css_class("edit-dragging");
    let label = gtk::Label::new(Some(&entry.display_name));
    label.add_css_class("heading");
    label.set_margin_start(12);
    label.set_margin_end(12);
    label.set_margin_top(6);
    label.set_margin_bottom(6);
    gtk::DragIcon::for_drag(drag).set_child(Some(&label));
    drag.set_hotspot(row.width() / 2, row.height() / 2);
}
