//! Compact (icon-only) sidebar with the original width and button animations.

use super::entries::Entry;
use super::{COMPACT_ANIM_DURATION, Inner, SIDEBAR_COMPACT_WIDTH, SIDEBAR_WIDTH};
use crate::config;
use adw::prelude::*;
use serde_json::Value;
use std::cell::Cell;
use std::rc::Rc;

/// Windows narrower than this switch to compact mode automatically.
const AUTO_COMPACT_WIDTH: i32 = 1000;

impl Inner {
    /// Original `_set_compact_mode`.
    pub(super) fn set_compact_mode(self: &Rc<Self>, compact: bool, animated: bool) {
        if self.sidebar().is_none() || self.compact_mode.get() == compact {
            return;
        }
        self.compact_mode.set(compact);
        if let (Some(python), Some(window)) = (&self.services.python, self.window()) {
            python.set_compact_mode(window.upcast_ref(), compact);
        }
        if compact {
            self.transition_to_compact(animated);
        } else {
            self.transition_to_normal(animated);
        }
    }

    /// Original `_on_window_width_changed` (`notify::default-width`).
    pub(super) fn on_window_width_changed(self: &Rc<Self>) {
        if self.edit_mode.get() {
            return;
        }
        let Some(window) = self.window() else {
            return;
        };
        let width = window.default_size().0;
        if width > 0 && width < AUTO_COMPACT_WIDTH {
            if !self.compact_mode.get() && !self.user_toggled_compact.get() {
                self.auto_compact_active.set(true);
                self.set_compact_mode(true, true);
            }
        } else {
            // Wide enough again: forget the manual override.
            self.user_toggled_compact.set(false);
            if self.auto_compact_active.get() {
                self.auto_compact_active.set(false);
                if !config::flag(&self.services.config.load(), config::COMPACT_MODE) {
                    self.set_compact_mode(false, true);
                }
            }
        }
    }

    /// Original `_on_compact_toggle_clicked`.
    pub(super) fn on_compact_toggle_clicked(self: &Rc<Self>) {
        let compact = !self.compact_mode.get();
        self.set_compact_mode(compact, true);
        self.services.config.update(|config| {
            config.insert(config::COMPACT_MODE.into(), Value::Bool(compact));
        });
        self.auto_compact_active.set(false);
        self.user_toggled_compact.set(true);
    }

    /// Original `_on_compact_icon_clicked`.
    pub(super) fn on_compact_icon_clicked(self: &Rc<Self>, entry: &Rc<Entry>) {
        self.select_entry(entry);
        self.update_compact_selection(Some(entry));
    }

    /// Original `_update_compact_selection`.
    pub(super) fn update_compact_selection(&self, selected: Option<&Entry>) {
        for entry in self.entries.borrow().iter() {
            if selected.is_some_and(|selected| std::ptr::eq(selected, entry.as_ref())) {
                entry.compact_button.add_css_class("compact-icon-selected");
            } else {
                entry
                    .compact_button
                    .remove_css_class("compact-icon-selected");
            }
        }
    }

    fn apply_sidebar_width(&self, width: i32) {
        if let Some(split_view) = &self.ui.split_view {
            split_view.set_min_sidebar_width(f64::from(width));
            split_view.set_max_sidebar_width(f64::from(width));
        }
    }

    /// Original `_animate_toggle_button`: rotate the compact toggle between
    /// 180° (normal) and 0° (compact).
    fn animate_toggle_button(self: &Rc<Self>, target_deg: f64, animated: bool) {
        if let Some(animation) = self.btn_rotate_anim.take() {
            animation.skip();
        }
        let Some(sidebar) = self.sidebar() else {
            return;
        };
        if !animated {
            self.btn_rotation_deg.set(target_deg);
            self.apply_btn_rotation(target_deg);
            return;
        }
        let weak = Rc::downgrade(self);
        let target = adw::CallbackAnimationTarget::new(move |value| {
            if let Some(this) = weak.upgrade() {
                this.btn_rotation_deg.set(value);
                this.apply_btn_rotation(value);
            }
        });
        let animation = adw::TimedAnimation::new(
            &sidebar.compact_button,
            self.btn_rotation_deg.get(),
            target_deg,
            COMPACT_ANIM_DURATION,
            target,
        );
        animation.set_easing(adw::Easing::EaseInOutCubic);
        self.btn_rotate_anim.replace(Some(animation.clone()));
        animation.play();
    }

    fn apply_btn_rotation(&self, deg: f64) {
        if let Some(sidebar) = self.sidebar() {
            sidebar.compact_button_css.load_from_string(&format!(
                ".compact-toggle-btn {{ transition: none; transform: rotate({deg}deg); }}"
            ));
        }
    }

    /// Run the sidebar width animation from `from` to `to`, calling `swap`
    /// once the width passes the midpoint (original `_transition_to_*`).
    fn animate_sidebar(
        self: &Rc<Self>,
        from: i32,
        to: i32,
        swap: fn(&Inner),
        clear_when_done: bool,
    ) {
        if let Some(animation) = self.sidebar_anim.take() {
            animation.skip();
        }
        let Some(split_view) = &self.ui.split_view else {
            return;
        };
        let mid = f64::from(SIDEBAR_WIDTH + SIDEBAR_COMPACT_WIDTH) / 2.0;
        let shrinking = to < from;
        let swapped = Cell::new(false);
        let weak = Rc::downgrade(self);
        let target = adw::CallbackAnimationTarget::new(move |value| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            this.apply_sidebar_width(value as i32);
            let passed = if shrinking {
                value <= mid
            } else {
                value >= mid
            };
            if !swapped.get() && passed {
                swapped.set(true);
                swap(&this);
            }
        });
        let animation = adw::TimedAnimation::new(
            split_view,
            f64::from(from),
            f64::from(to),
            COMPACT_ANIM_DURATION,
            target,
        );
        animation.set_easing(adw::Easing::EaseInOutCubic);
        if clear_when_done {
            let weak = Rc::downgrade(self);
            animation.connect_done(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.sidebar_anim.take();
                }
            });
        }
        self.sidebar_anim.replace(Some(animation.clone()));
        animation.play();
    }

    /// Original `_transition_to_compact`.
    fn transition_to_compact(self: &Rc<Self>, animated: bool) {
        let selected = self
            .ui
            .widget_list
            .selected_row()
            .and_then(|row| self.entry_for_row(&row));
        if let Some(entry) = selected {
            self.update_compact_selection(Some(&entry));
        }
        self.animate_toggle_button(0.0, animated);
        if animated {
            self.animate_sidebar(
                SIDEBAR_WIDTH,
                SIDEBAR_COMPACT_WIDTH,
                Inner::swap_to_compact,
                false,
            );
        } else {
            self.swap_to_compact();
            self.apply_sidebar_width(SIDEBAR_COMPACT_WIDTH);
        }
    }

    /// Original `_transition_to_normal`.
    fn transition_to_normal(self: &Rc<Self>, animated: bool) {
        self.animate_toggle_button(180.0, animated);
        if animated {
            self.animate_sidebar(
                SIDEBAR_COMPACT_WIDTH,
                SIDEBAR_WIDTH,
                Inner::swap_to_normal,
                true,
            );
        } else {
            self.apply_sidebar_width(SIDEBAR_WIDTH);
            self.swap_to_normal();
        }
    }

    fn swap_to_compact(&self) {
        if let Some(sidebar) = self.sidebar() {
            sidebar.title.set_visible(false);
            sidebar.list_container.set_visible(false);
            sidebar.compact_scroll.set_visible(true);
            sidebar.compact_strip.set_visible(true);
        }
    }

    fn swap_to_normal(&self) {
        if let Some(sidebar) = self.sidebar() {
            sidebar.title.set_visible(!self.services.kinexin);
            sidebar.list_container.set_visible(true);
            sidebar.compact_scroll.set_visible(false);
            sidebar.compact_strip.set_visible(false);
        }
    }
}
