//! Command lock (original `CommandLockManager` UI side) and close handling.

use super::Inner;
use super::entries::{Instance, LoadState, Source};
use adw::prelude::*;
use gtk::glib;
use linexin_services::lock;
use std::rc::Rc;

impl Inner {
    /// Route the shared lock to this window (the newest window wins, as the
    /// original replaced its global lock manager per window).
    pub(super) fn register_lock_callback(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        lock::set_apply_callback(move |locked| {
            if let Some(this) = weak.upgrade() {
                this.apply_command_lock(locked);
            }
        });
    }

    /// Original `_apply_command_lock`.
    fn apply_command_lock(&self, locked: bool) {
        self.is_command_locked.set(locked);
        self.ui.widget_list.set_sensitive(!locked);
        if let Some(sidebar) = self.sidebar() {
            set_window_controls_disabled(&sidebar.header, locked);
        }
        set_window_controls_disabled(&self.ui.content_header, locked);
        if let Some(split_view) = &self.ui.split_view {
            split_view.set_show_content(!locked);
        }
    }

    /// Original `do_close_request` (+ the close blocking of `_block_wm_close`).
    pub(super) fn on_close_request(&self) -> glib::Propagation {
        if self.is_command_locked.get() {
            return glib::Propagation::Stop;
        }
        if let Some(python) = &self.services.python {
            python.close_request();
        }
        let natives: Vec<_> = self
            .entries
            .borrow()
            .iter()
            .filter_map(|entry| match (&entry.source, &*entry.state.borrow()) {
                (Source::Native(native), LoadState::Loaded(Instance::Native(widget))) => {
                    Some((native.clone(), widget.clone()))
                }
                _ => None,
            })
            .collect();
        for (native, widget) in natives {
            native.close_request(&widget);
        }
        glib::Propagation::Proceed
    }
}

/// Hide window controls but keep their allocation, and make them untargetable.
fn set_window_controls_disabled(header: &gtk::HeaderBar, disabled: bool) {
    for controls in find_window_controls(header.upcast_ref()) {
        controls.set_opacity(if disabled { 0.0 } else { 1.0 });
        controls.set_can_target(!disabled);
    }
}

fn find_window_controls(widget: &gtk::Widget) -> Vec<gtk::WindowControls> {
    let mut found = vec![];
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(controls) = current.downcast_ref::<gtk::WindowControls>() {
            found.push(controls.clone());
        }
        found.extend(find_window_controls(&current));
        child = current.next_sibling();
    }
    found
}
