//! Widget discovery, deferred loading, background preloading and selection
//! (original `_load_all_widgets*`, `_start_bg_import`, `_finish_bg_load`,
//! `_execute_widget_selection`, single-widget `_staged_load_step*`).

use super::Inner;
use super::entries::{Entry, Instance, LoadState, Metadata, Source, default_widget_id};
use crate::catalog::{self, Kind, WidgetFile};
use crate::config;
use crate::native::NativeWidget;
use adw::prelude::*;
use gtk::glib;
use linexin_localization::{global, tr};
use linexin_python_bridge::{LoadedWidget, Module, PythonRuntime};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::rc::{Rc, Weak};

/// Widget the original selects on the very first launch.
const FIRST_LAUNCH_WIDGET: &str = "Linexin Hello";

/// Let the main loop redraw (spinner) and handle input before slow work on
/// the GTK thread, as the original did around Python widget creation.
fn flush_main_context() {
    let context = glib::MainContext::default();
    while context.pending() {
        context.iteration(false);
    }
}

impl Inner {
    /// Original `_load_all_widgets` for the sidebar mode.
    pub(super) fn load_all_widgets(self: &Rc<Self>) {
        let dirs = &self.services.paths.widgets_dirs;
        if !catalog::any_exists(dirs) {
            let dir = dirs
                .first()
                .map(|d| d.display().to_string())
                .unwrap_or_default();
            self.show_error_in_list(
                &tr("No widgets found"),
                &tr("Directory does not exist").replace("Directory", &dir),
            );
            return;
        }
        let saved = config::names(&self.services.config.load(), config::WIDGET_ORDER);
        for file in catalog::sidebar_order(dirs, saved) {
            self.register_file(&file);
        }
        self.auto_select_first_widget();
        self.start_background_preload();
    }

    /// Original `refresh_widgets`: register files that appeared since startup.
    pub(super) fn refresh_widgets(self: &Rc<Self>) {
        let dirs = &self.services.paths.widgets_dirs;
        if !catalog::any_exists(dirs) {
            return;
        }
        // Stems, so a .so installed over a loaded .py does not appear twice.
        let known: HashSet<String> = self
            .entries
            .borrow()
            .iter()
            .map(|entry| catalog::file_stem(&entry.file_name).to_owned())
            .collect();
        let mut new_files: Vec<WidgetFile> = catalog::discover(dirs)
            .into_iter()
            .filter(|file| !known.contains(file.stem()))
            .collect();
        catalog::sort_by_mtime(&mut new_files);
        for file in &new_files {
            self.register_file(file);
        }
    }

    fn python(&self) -> Option<&Rc<PythonRuntime>> {
        let python = self.services.python.as_ref();
        if python.is_none() && !self.python_disabled_logged.replace(true) {
            eprintln!("Python widgets are skipped: the Python runtime is unavailable");
        }
        python
    }

    /// Native descriptors and scanned Python metadata become deferred entries;
    /// Python files that cannot be scanned are imported right away (original
    /// fallback).
    fn register_file(self: &Rc<Self>, file: &WidgetFile) {
        match file.kind {
            Kind::Native => match NativeWidget::load(&file.path) {
                Ok(native) => {
                    self.add_native_entry(file, native);
                }
                Err(error) => {
                    eprintln!(
                        "Failed to load native widget {}: {error}",
                        file.path.display()
                    );
                    let name = file.stem().to_owned();
                    let meta = Metadata {
                        widget_id: default_widget_id(&name),
                        widget_name: name,
                        icon: None,
                    };
                    let state = LoadState::Failed(error);
                    self.add_entry(
                        &file.file_name,
                        file.path.clone(),
                        Source::Broken,
                        meta,
                        state,
                    );
                }
            },
            Kind::Python => {
                let Some(python) = self.python().cloned() else {
                    return;
                };
                match python.scan(&file.path) {
                    Some(scanned) => {
                        let meta = Metadata {
                            widget_name: scanned.widget_name,
                            widget_id: scanned.widget_id,
                            icon: scanned.icon_path,
                        };
                        let source = Source::Python {
                            class_name: Some(scanned.class_name),
                        };
                        self.add_entry(
                            &file.file_name,
                            file.path.clone(),
                            source,
                            meta,
                            LoadState::Deferred,
                        );
                    }
                    None => match python.import_module(&file.path) {
                        Ok(module) => self.instantiate_python(
                            &python,
                            None,
                            &file.file_name,
                            &file.path,
                            &module,
                        ),
                        Err(error) => {
                            eprintln!("Failed to import {}: {error}", file.path.display())
                        }
                    },
                }
            }
        }
    }

    fn add_native_entry(self: &Rc<Self>, file: &WidgetFile, native: NativeWidget) -> Rc<Entry> {
        let meta = Metadata {
            widget_name: native.name.clone(),
            widget_id: native.widget_id.clone(),
            icon: Some(native.icon.clone()).filter(|icon| !icon.is_empty()),
        };
        let source = Source::Native(Rc::new(native));
        self.add_entry(
            &file.file_name,
            file.path.clone(),
            source,
            meta,
            LoadState::Deferred,
        )
    }

    /// Construct the widget classes of an imported module (original
    /// `_extract_widgets_from_module` + `_register_widget`). The instance of
    /// the scanned class completes `deferred`; other classes get own entries.
    fn instantiate_python(
        self: &Rc<Self>,
        python: &PythonRuntime,
        deferred: Option<&Rc<Entry>>,
        file_name: &str,
        path: &Path,
        module: &Module,
    ) {
        let Some(window) = self.window() else {
            return;
        };
        let mut widgets: Vec<LoadedWidget> = vec![];
        for result in python.instantiate(module, window.upcast_ref(), &self.widget_args()) {
            match result {
                Ok(loaded) => widgets.push(loaded),
                Err(error) => println!("Failed to instantiate widget in {file_name}: {error}"),
            }
        }
        if let Some(entry) = deferred {
            let class_name = match &entry.source {
                Source::Python { class_name } => class_name.clone(),
                _ => None,
            };
            let name = entry.widget_name();
            let index = widgets
                .iter()
                .position(|w| class_name.as_ref() == Some(&w.class_name))
                .or_else(|| widgets.iter().position(|w| w.widget_name == name))
                .or_else(|| (!widgets.is_empty()).then_some(0));
            if let Some(index) = index {
                let loaded = widgets.remove(index);
                {
                    let mut meta = entry.meta.borrow_mut();
                    meta.widget_id = loaded.widget_id.clone();
                    meta.icon = loaded.icon_path.clone();
                }
                entry
                    .state
                    .replace(LoadState::Loaded(Instance::Python(loaded)));
            }
        }
        for loaded in widgets {
            let duplicate = self
                .entries
                .borrow()
                .iter()
                .any(|entry| entry.widget_name() == loaded.widget_name);
            if duplicate {
                println!(
                    "Ignoring a second widget named {:?} in {file_name}",
                    loaded.widget_name
                );
                continue;
            }
            let meta = Metadata {
                widget_name: loaded.widget_name.clone(),
                widget_id: loaded.widget_id.clone(),
                icon: loaded.icon_path.clone(),
            };
            let source = Source::Python {
                class_name: Some(loaded.class_name.clone()),
            };
            let state = LoadState::Loaded(Instance::Python(loaded));
            self.add_entry(file_name, path.to_path_buf(), source, meta, state);
        }
    }

    /// Create a native widget on the GTK thread (they are cheap to build).
    fn create_native(&self, entry: &Entry) {
        let Source::Native(native) = &entry.source else {
            return;
        };
        let Some(window) = self.window() else {
            return;
        };
        let state = match native.create(
            window.upcast_ref(),
            self.options.hide_sidebar(),
            self.options.voice_autostart,
            self.options.conversation_id.as_deref(),
        ) {
            Ok(widget) => LoadState::Loaded(Instance::Native(widget)),
            Err(error) => {
                eprintln!(
                    "Failed to create native widget {}: {error}",
                    native.path.display()
                );
                LoadState::Failed(error)
            }
        };
        entry.state.replace(state);
    }

    /// Original `_auto_select_first_widget`: Linexin Hello on the first launch,
    /// otherwise the first widget. Hidden widgets are skipped.
    fn auto_select_first_widget(self: &Rc<Self>) {
        if self.first_widget_loaded.get() {
            return;
        }
        let mut config = self.services.config.load();
        if !config::flag(&config, config::HAS_LAUNCHED_BEFORE) {
            config.insert(config::HAS_LAUNCHED_BEFORE.into(), Value::Bool(true));
            self.services.config.save(&config);
            let hello = self
                .entries
                .borrow()
                .iter()
                .find(|entry| entry.widget_name() == FIRST_LAUNCH_WIDGET)
                .cloned();
            if let Some(entry) = hello {
                self.select_entry(&entry);
                self.first_widget_loaded.set(true);
                return;
            }
        }
        let first = self
            .entries
            .borrow()
            .iter()
            .find(|entry| !entry.hidden.get())
            .cloned();
        if let Some(entry) = first {
            self.select_entry(&entry);
            self.first_widget_loaded.set(true);
        }
    }

    /// Original `_auto_select_first_visible_widget`.
    pub(super) fn auto_select_first_visible_widget(self: &Rc<Self>) {
        let first = self
            .entries
            .borrow()
            .iter()
            // get_visible, like the original's row.get_visible(): is_visible
            // would also need mapped ancestors and fails in compact mode.
            .find(|entry| entry.row.get_visible() && !entry.hidden.get())
            .cloned();
        if let Some(entry) = first {
            self.select_entry(&entry);
        }
    }

    pub(super) fn select_entry(self: &Rc<Self>, entry: &Rc<Entry>) {
        self.ui.widget_list.select_row(Some(&entry.row));
        self.on_widget_selected(entry);
    }

    /// Original `_on_widget_selected`: debounce selections by 50 ms.
    pub(super) fn on_widget_selected(self: &Rc<Self>, entry: &Rc<Entry>) {
        if self.is_command_locked.get() {
            return;
        }
        if let Some(source) = self.selection_timeout.take() {
            source.remove();
        }
        self.pending_selection.replace(Some(Rc::downgrade(entry)));
        let source = self.after(50, Inner::execute_widget_selection);
        self.selection_timeout.replace(Some(source));
    }

    /// Original `_execute_widget_selection`.
    fn execute_widget_selection(self: &Rc<Self>) {
        self.selection_timeout.take();
        let Some(entry) = self.find_entry(&self.pending_selection.borrow()) else {
            return;
        };
        let deferred = matches!(
            *entry.state.borrow(),
            LoadState::Deferred | LoadState::Importing
        );
        if deferred {
            if matches!(entry.source, Source::Python { .. }) {
                self.load_deferred_widget(&entry);
                return;
            }
            self.create_native(&entry);
        }
        // The latest pick wins: a widget still loading from an earlier pick
        // must not take over the content when it finishes. (The original
        // could, whenever the newer pick had finished loading first.)
        self.activate_after_load.take();
        self.set_active(&entry);
        let page_name = catalog::stack_page_name(&entry.file_name);
        self.add_widget_to_stack(&entry, &page_name);
        self.switch_to_page(&entry, &page_name);
        self.ui.content_page.set_title(&entry.display_name);
        if self.compact_mode.get() {
            self.update_compact_selection(Some(&entry));
        }
    }

    fn set_active(&self, entry: &Rc<Entry>) {
        self.active_entry.replace(Some(Rc::downgrade(entry)));
        if let (Some(python), Some(window)) = (&self.services.python, self.window()) {
            python.set_active_widget_id(window.upcast_ref(), Some(&entry.widget_id()));
        }
    }

    /// Slide down to a widget below the visible one, up to one above it.
    fn switch_to_page(&self, entry: &Rc<Entry>, page_name: &str) {
        let previous = self.find_entry(&self.visible_entry.borrow());
        let transition = previous.and_then(|previous| {
            let (from, to) = (self.index_of(&previous)?, self.index_of(entry)?);
            match to.cmp(&from) {
                Ordering::Greater => Some(gtk::StackTransitionType::SlideDown),
                Ordering::Less => Some(gtk::StackTransitionType::SlideUp),
                Ordering::Equal => None,
            }
        });
        let stack = &self.ui.widget_stack;
        match transition {
            Some(transition) => stack.set_visible_child_full(page_name, transition),
            None => stack.set_visible_child_name(page_name),
        }
        self.visible_entry.replace(Some(Rc::downgrade(entry)));
    }

    /// Original `_load_deferred_widget`: spinner page, then import now unless
    /// a background import is already running.
    fn load_deferred_widget(self: &Rc<Self>, entry: &Rc<Entry>) {
        self.show_loading_spinner(entry);
        self.activate_after_load.replace(Some(Rc::downgrade(entry)));
        if matches!(*entry.state.borrow(), LoadState::Importing) {
            return;
        }
        self.preload_queue
            .borrow_mut()
            .retain(|queued| queued.as_ptr() != Rc::as_ptr(entry));
        self.start_bg_import(entry);
    }

    /// Original `_start_background_preload`.
    fn start_background_preload(self: &Rc<Self>) {
        let queue: VecDeque<Weak<Entry>> = self
            .entries
            .borrow()
            .iter()
            .filter(|entry| matches!(*entry.state.borrow(), LoadState::Deferred))
            .map(Rc::downgrade)
            .collect();
        let empty = queue.is_empty();
        self.preload_queue.replace(queue);
        if !empty {
            self.after(200, Inner::preload_next);
        }
    }

    /// Original `_preload_next`: one import at a time.
    fn preload_next(self: &Rc<Self>) {
        let importing = self
            .entries
            .borrow()
            .iter()
            .any(|entry| matches!(*entry.state.borrow(), LoadState::Importing));
        if importing {
            return;
        }
        loop {
            let next = self.preload_queue.borrow_mut().pop_front();
            let Some(next) = next else {
                return;
            };
            if let Some(entry) = next.upgrade()
                && matches!(*entry.state.borrow(), LoadState::Deferred)
            {
                self.start_bg_import(&entry);
                return;
            }
        }
    }

    /// Original `_start_bg_import`; native widgets are created directly.
    fn start_bg_import(self: &Rc<Self>, entry: &Rc<Entry>) {
        if !matches!(entry.source, Source::Python { .. }) {
            self.create_native(entry);
            self.finish_load(entry);
            return;
        }
        let Some(python) = self.python().cloned() else {
            entry.state.replace(LoadState::Failed(
                "the Python runtime is unavailable".into(),
            ));
            self.finish_load(entry);
            return;
        };
        entry.state.replace(LoadState::Importing);
        let (weak_self, weak_entry) = (Rc::downgrade(self), Rc::downgrade(entry));
        python.import_in_background(entry.path.clone(), move |result| {
            if let (Some(this), Some(entry)) = (weak_self.upgrade(), weak_entry.upgrade()) {
                this.finish_bg_load(&entry, result);
            }
        });
    }

    /// Original `_finish_bg_load` (GTK thread).
    fn finish_bg_load(self: &Rc<Self>, entry: &Rc<Entry>, result: Result<Module, String>) {
        if matches!(*entry.state.borrow(), LoadState::Importing) {
            match (result, self.services.python.clone()) {
                (Ok(module), Some(python)) => {
                    flush_main_context();
                    self.instantiate_python(
                        &python,
                        Some(entry),
                        &entry.file_name,
                        &entry.path,
                        &module,
                    );
                    flush_main_context();
                    if !entry.is_loaded() {
                        entry
                            .state
                            .replace(LoadState::Failed(tr("Widget cannot be displayed")));
                    }
                }
                (Err(error), _) => {
                    eprintln!("Failed to import {}: {error}", entry.path.display());
                    entry.state.replace(LoadState::Failed(error));
                }
                (Ok(_), None) => {
                    entry.state.replace(LoadState::Failed(
                        "the Python runtime is unavailable".into(),
                    ));
                }
            }
        }
        self.finish_load(entry);
    }

    /// Drop the spinner, show the widget if the user is waiting for it and
    /// continue preloading.
    fn finish_load(self: &Rc<Self>, entry: &Rc<Entry>) {
        self.remove_loading_spinner(entry);
        let requested = self
            .activate_after_load
            .borrow()
            .as_ref()
            .is_some_and(|wanted| wanted.as_ptr() == Rc::as_ptr(entry));
        if requested {
            self.activate_after_load.take();
            self.activate_preloaded_widget(entry);
        }
        self.after(50, Inner::preload_next);
    }

    /// Original `_activate_preloaded_widget` (also shows load errors).
    fn activate_preloaded_widget(&self, entry: &Rc<Entry>) {
        self.set_active(entry);
        let page_name = catalog::stack_page_name(&entry.file_name);
        self.add_widget_to_stack(entry, &page_name);
        self.ui.widget_stack.set_visible_child_name(&page_name);
        self.visible_entry.replace(Some(Rc::downgrade(entry)));
        self.ui.content_page.set_title(&entry.display_name);
        if self.compact_mode.get() {
            self.update_compact_selection(Some(entry));
        }
    }

    /// Original `_staged_load_step1` (single-widget mode, 50 ms after start).
    pub(super) fn staged_load(self: &Rc<Self>) {
        let Some(target) = self.options.target_widget.clone() else {
            return;
        };
        let files = catalog::discover(&self.services.paths.widgets_dirs);
        let Some(file) = catalog::find_target(&files, &target).cloned() else {
            println!("Target widget file not found: {target}");
            self.show_single_widget_error();
            return;
        };
        println!("Loading target widget: {}", file.file_name);
        match file.kind {
            Kind::Native => {
                match NativeWidget::load(&file.path) {
                    Ok(native) => {
                        let entry = self.add_native_entry(&file, native);
                        self.create_native(&entry);
                    }
                    Err(error) => {
                        eprintln!(
                            "Failed to load native widget {}: {error}",
                            file.path.display()
                        );
                    }
                }
                self.select_target_widget();
            }
            Kind::Python => {
                let Some(python) = self.python().cloned() else {
                    self.select_target_widget();
                    return;
                };
                let weak = Rc::downgrade(self);
                python.import_in_background(file.path.clone(), move |result| {
                    if let Some(this) = weak.upgrade() {
                        this.staged_load_step2(&file, result);
                    }
                });
            }
        }
    }

    /// Original `_staged_load_step2` (GTK thread).
    fn staged_load_step2(self: &Rc<Self>, file: &WidgetFile, result: Result<Module, String>) {
        match (result, self.services.python.clone()) {
            (Ok(module), Some(python)) => {
                flush_main_context();
                self.instantiate_python(&python, None, &file.file_name, &file.path, &module);
                flush_main_context();
            }
            (Err(error), _) => eprintln!("Failed to import {}: {error}", file.path.display()),
            (Ok(_), None) => {}
        }
        self.select_target_widget();
    }

    /// Original `_select_target_widget`.
    fn select_target_widget(self: &Rc<Self>) {
        let first = self
            .entries
            .borrow()
            .iter()
            .find(|entry| entry.is_loaded())
            .cloned();
        match first {
            Some(entry) => {
                println!("Loading single widget: {}", entry.widget_name());
                self.load_single_widget_mode(&entry);
            }
            None => {
                let target = self.options.target_widget.as_deref().unwrap_or_default();
                println!("No widgets loaded for target: {target}");
                self.show_single_widget_error();
            }
        }
    }

    /// Original `_load_single_widget_mode`.
    fn load_single_widget_mode(self: &Rc<Self>, entry: &Rc<Entry>) {
        self.set_active(entry);
        self.idle(|this| this.update_widget_header());
        let page_name = catalog::stack_page_name(&entry.file_name);
        self.add_widget_to_stack(entry, &page_name);
        self.switch_to_page(entry, &page_name);
        let display_name = global().get_text(&entry.widget_name(), &entry.widget_id());
        if let Some(window) = self.window() {
            window.set_title(Some(&display_name));
        }
        self.ui.content_page.set_title(&display_name);
    }

    /// Original `_show_error_in_list`.
    fn show_error_in_list(&self, title: &str, subtitle: &str) {
        let row = adw::ActionRow::new();
        row.set_title(title);
        row.set_subtitle(subtitle);
        self.ui.widget_list.append(&row);
    }
}
