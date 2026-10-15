//! `_linexin_host`: the Rust services the embedded bridge exposes to Python.
//!
//! Python widgets call these from worker threads too. None of them holds a
//! Rust lock while attached to Python, and anything that can block or wait
//! for another thread runs detached from the interpreter.

use std::ffi::OsString;

use gtk::glib;
use linexin_localization as localization;
use linexin_services::{lock, sudo};
use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register(py: Python<'_>) -> PyResult<()> {
    let module = PyModule::new(py, "_linexin_host")?;
    module.add_function(wrap_pyfunction!(translate, &module)?)?;
    module.add_function(wrap_pyfunction!(get_text, &module)?)?;
    module.add_function(wrap_pyfunction!(has_domain, &module)?)?;
    module.add_function(wrap_pyfunction!(domain_for_module, &module)?)?;
    module.add_function(wrap_pyfunction!(translate_dialog, &module)?)?;
    module.add_function(wrap_pyfunction!(translate_window, &module)?)?;
    module.add_function(wrap_pyfunction!(lock_ui, &module)?)?;
    module.add_function(wrap_pyfunction!(unlock_ui, &module)?)?;
    module.add_function(wrap_pyfunction!(is_locked, &module)?)?;
    module.add_function(wrap_pyfunction!(refresh_widgets, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_paths, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_password, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_has_password, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_set_password, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_validate_password, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_allow_feed, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_drain, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_start_session, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_stop_session, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_clear_cache, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_forget_password, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_cleanup, &module)?)?;
    module.add_function(wrap_pyfunction!(sudo_is_wrapper_command, &module)?)?;
    py.import("sys")?
        .getattr("modules")?
        .set_item("_linexin_host", module)
}

/// Original `_translate_with_patterns_helper(text, domain)`.
#[pyfunction]
fn translate(py: Python<'_>, text: String, domain: String) -> String {
    py.detach(move || localization::global().translate(&text, &domain))
}

/// Original `WidgetLocalizationManager.get_text(key, domain)`.
#[pyfunction]
fn get_text(py: Python<'_>, key: String, domain: String) -> String {
    py.detach(move || localization::global().get_text(&key, &domain))
}

#[pyfunction]
fn has_domain(py: Python<'_>, domain: String) -> bool {
    py.detach(move || localization::global().has_domain(&domain))
}

/// Original `_prepare_module` dictionary key for a widget file stem.
#[pyfunction]
fn domain_for_module(py: Python<'_>, stem: String) -> String {
    py.detach(move || localization::global().domain_for_module(&stem))
}

/// A widget handed to [`Python::detach`], whose closure runs on this thread.
struct SameThread(gtk::Widget);

// SAFETY: only moved into `Python::detach`, which calls its closure on the
// current thread before returning.
unsafe impl Send for SameThread {}

impl SameThread {
    fn get(&self) -> &gtk::Widget {
        &self.0
    }
}

/// Translate a dialog's heading/body and widget tree; a no-op for anything
/// that is not a GTK widget (like the original's attribute probing).
#[pyfunction]
fn translate_dialog(py: Python<'_>, dialog: &Bound<'_, PyAny>, domain: String) -> PyResult<()> {
    if let Some(widget) = crate::widget_from_python(dialog)? {
        let widget = SameThread(widget);
        py.detach(move || localization::widgets::translate_dialog(widget.get(), &domain));
    }
    Ok(())
}

#[pyfunction]
fn translate_window(py: Python<'_>, window: &Bound<'_, PyAny>, domain: String) -> PyResult<()> {
    if let Some(widget) = crate::widget_from_python(window)? {
        let widget = SameThread(widget);
        py.detach(move || localization::widgets::translate_widget_tree(widget.get(), &domain));
    }
    Ok(())
}

#[pyfunction(name = "lock")]
fn lock_ui() {
    lock::lock();
}

#[pyfunction(name = "unlock")]
fn unlock_ui() {
    lock::unlock();
}

#[pyfunction]
fn is_locked() -> bool {
    lock::is_locked()
}

/// `window.refresh_widgets()`: the host hook runs on the GTK main thread,
/// directly when called there (like the original method call).
#[pyfunction]
fn refresh_widgets(py: Python<'_>, window: usize) {
    if crate::on_main_thread() {
        py.detach(move || crate::refresh_widgets(window));
    } else {
        glib::idle_add_once(move || crate::refresh_widgets(window));
    }
}

/// (wrapper, askpass, FIFO) paths; creates the shared manager like the
/// original `get_sudo_manager()`.
#[pyfunction]
fn sudo_paths() -> (OsString, OsString, OsString) {
    let manager = sudo::manager();
    (
        manager.wrapper_path().into(),
        manager.askpass_path().into(),
        manager.fifo_path().into(),
    )
}

#[pyfunction]
fn sudo_password() -> Option<String> {
    sudo::manager().password()
}

#[pyfunction]
fn sudo_has_password() -> bool {
    sudo::manager().has_password()
}

#[pyfunction]
fn sudo_set_password(password: String) {
    sudo::manager().set_password(&password);
}

#[pyfunction]
fn sudo_validate_password(py: Python<'_>, password: String) -> bool {
    py.detach(move || sudo::manager().validate_password(&password))
}

/// May wait for another caller's one-shot grant to be drained.
#[pyfunction]
fn sudo_allow_feed(py: Python<'_>) {
    py.detach(|| sudo::manager().allow_feed());
}

#[pyfunction]
fn sudo_drain(py: Python<'_>) {
    py.detach(|| sudo::manager().drain());
}

#[pyfunction]
fn sudo_start_session() {
    sudo::manager().start_session();
}

#[pyfunction]
fn sudo_stop_session(py: Python<'_>) {
    py.detach(|| sudo::manager().stop_session());
}

#[pyfunction]
fn sudo_clear_cache(py: Python<'_>) {
    py.detach(|| sudo::manager().clear_cache());
}

#[pyfunction]
fn sudo_forget_password(py: Python<'_>) {
    py.detach(|| sudo::manager().forget_password());
}

#[pyfunction]
fn sudo_cleanup(py: Python<'_>) {
    py.detach(|| sudo::manager().cleanup());
}

/// Original `_should_lock_command` wrapper test; false until the manager
/// exists (original `SudoManager._instance`).
#[pyfunction]
fn sudo_is_wrapper_command(command: OsString) -> bool {
    sudo::existing().is_some_and(|manager| manager.is_wrapper_command(&command.to_string_lossy()))
}
