//! Port of the original `WidgetLoaderWindow`.
//!
//! [`CenterWindow`] owns the state in an `Rc`; widget callbacks, timeouts and
//! animations hold only weak references to it, and the state holds only a weak
//! reference to the `GtkWindow`, so nothing forms a reference cycle. The
//! application keeps every `CenterWindow` until its window is destroyed.

mod compact;
mod content;
mod edit;
mod entries;
mod loading;
mod lock;
mod shortcuts;
mod sidebar;
mod style;

use crate::config::{self, ConfigFile};
use crate::paths::Paths;
use adw::prelude::*;
use entries::{Entry, Instance, LoadState, Source};
use gtk::{gio, glib};
use linexin_localization::tr;
use linexin_python_bridge::{PythonRuntime, WidgetArgs, WindowHooks};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};
use std::time::Duration;

pub const SIDEBAR_WIDTH: i32 = 330;
pub const SIDEBAR_COMPACT_WIDTH: i32 = 62;
pub const ICON_SIZE: i32 = 32;
pub const COMPACT_ANIM_DURATION: u32 = 350;

/// Process-wide services shared by every window.
pub struct Services {
    pub paths: Paths,
    pub config: ConfigFile,
    pub python: Option<Rc<PythonRuntime>>,
    /// Original `IS_PLASMA_OR_HYPRLAND`.
    pub kinexin: bool,
}

/// Per-window command line settings (original constructor arguments).
#[derive(Clone, Debug, Default)]
pub struct WindowOptions {
    pub target_widget: Option<String>,
    pub voice_autostart: bool,
    pub conversation_id: Option<String>,
}

impl WindowOptions {
    pub fn hide_sidebar(&self) -> bool {
        self.target_widget.is_some()
    }
}

/// Handle to a Linexin Center window.
#[derive(Clone)]
pub struct CenterWindow(Rc<Inner>);

thread_local! {
    /// Every open Center window, for a native widget's `refresh_widgets`.
    static WINDOWS: RefCell<Vec<Weak<Inner>>> = const { RefCell::new(Vec::new()) };
}

/// Pick up newly installed widgets in every window (GTK main thread).
pub(crate) fn refresh_all() {
    let windows: Vec<Rc<Inner>> =
        WINDOWS.with(|windows| windows.borrow().iter().filter_map(Weak::upgrade).collect());
    for window in windows {
        window.refresh_widgets();
    }
}

/// Whether the Center window behind `window` is in compact mode (the legacy
/// `window._compact_mode`); `false` for any other window.
pub(crate) fn compact_mode_of(window: *mut gtk::ffi::GtkWindow) -> bool {
    WINDOWS.with(|windows| {
        windows
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .any(|inner| {
                inner.compact_mode.get()
                    && inner
                        .window
                        .upgrade()
                        .is_some_and(|own| own.upcast_ref::<gtk::Window>().as_ptr() == window)
            })
    })
}

pub(crate) struct Inner {
    services: Rc<Services>,
    options: WindowOptions,
    window: glib::WeakRef<gtk::ApplicationWindow>,
    ui: Ui,
    /// Sidebar order (original `_sidebar_rows` / `widget_order`).
    entries: RefCell<Vec<Rc<Entry>>>,
    button_layout: RefCell<String>,
    wm_settings: Option<gio::Settings>,
    hidden_widgets: RefCell<Vec<String>>,

    // Loading and selection.
    preload_queue: RefCell<VecDeque<Weak<Entry>>>,
    activate_after_load: RefCell<Option<Weak<Entry>>>,
    loading_spinner_page: RefCell<Option<(String, Weak<Entry>)>>,
    visible_entry: RefCell<Option<Weak<Entry>>>,
    active_entry: RefCell<Option<Weak<Entry>>>,
    first_widget_loaded: Cell<bool>,
    selection_timeout: RefCell<Option<glib::SourceId>>,
    pending_selection: RefCell<Option<Weak<Entry>>>,
    python_disabled_logged: Cell<bool>,

    // Compact and edit modes.
    compact_mode: Cell<bool>,
    auto_compact_active: Cell<bool>,
    user_toggled_compact: Cell<bool>,
    edit_mode: Cell<bool>,
    edit_was_auto_compact: Cell<bool>,
    sidebar_anim: RefCell<Option<adw::TimedAnimation>>,
    btn_rotate_anim: RefCell<Option<adw::TimedAnimation>>,
    btn_rotation_deg: Cell<f64>,
    edit_shrink_provider: RefCell<Option<gtk::CssProvider>>,

    // Command lock, context menu and single-widget header.
    is_command_locked: Cell<bool>,
    context_menu_entry: RefCell<Option<Weak<Entry>>>,
    widget_header_widget: RefCell<Option<gtk::Widget>>,
}

/// Widgets that live as long as the window.
struct Ui {
    widget_stack: gtk::Stack,
    content_display: adw::Bin,
    widget_list: gtk::ListBox,
    content_page: adw::NavigationPage,
    content_header: gtk::HeaderBar,
    sidebar: Option<sidebar::Sidebar>,
    split_view: Option<adw::NavigationSplitView>,
}

impl CenterWindow {
    /// Build the window (not yet presented) and start loading widgets.
    pub fn new(
        application: &adw::Application,
        services: Rc<Services>,
        options: WindowOptions,
    ) -> Self {
        let window: gtk::ApplicationWindow = if services.kinexin {
            gtk::ApplicationWindow::new(application)
        } else {
            adw::ApplicationWindow::new(application).upcast()
        };
        window.set_title(Some(&tr("Linexin Center")));
        if options.hide_sidebar() {
            window.set_default_size(800, 600);
        } else {
            window.set_default_size(1280, 730);
        }
        let saved = services.config.load();
        let wm_settings = sidebar::wm_preferences();
        let button_layout = sidebar::button_layout(wm_settings.as_ref());
        let inner = Rc::new_cyclic(|weak: &Weak<Inner>| {
            let ui = Ui::build(weak, &window, &services, &options, &button_layout);
            Inner {
                window: window.downgrade(),
                ui,
                entries: RefCell::default(),
                button_layout: RefCell::new(button_layout),
                wm_settings,
                hidden_widgets: RefCell::new(config::names(&saved, config::HIDDEN_WIDGETS)),
                preload_queue: RefCell::default(),
                activate_after_load: RefCell::default(),
                loading_spinner_page: RefCell::default(),
                visible_entry: RefCell::default(),
                active_entry: RefCell::default(),
                first_widget_loaded: Cell::new(false),
                selection_timeout: RefCell::default(),
                pending_selection: RefCell::default(),
                python_disabled_logged: Cell::new(false),
                compact_mode: Cell::new(false),
                auto_compact_active: Cell::new(false),
                user_toggled_compact: Cell::new(false),
                edit_mode: Cell::new(false),
                edit_was_auto_compact: Cell::new(false),
                sidebar_anim: RefCell::default(),
                btn_rotate_anim: RefCell::default(),
                btn_rotation_deg: Cell::new(180.0),
                edit_shrink_provider: RefCell::default(),
                is_command_locked: Cell::new(false),
                context_menu_entry: RefCell::default(),
                widget_header_widget: RefCell::default(),
                services,
                options,
            }
        });
        inner.connect_window_signals(&window);
        inner.install_python_window(&window);
        style::apply(&window, inner.services.kinexin);

        if inner.options.hide_sidebar() {
            inner.after(50, Inner::staged_load);
        } else {
            inner.load_all_widgets();
            if config::flag(&saved, config::COMPACT_MODE) {
                inner.idle(|this| this.set_compact_mode(true, false));
            }
        }
        WINDOWS.with(|windows| {
            let mut windows = windows.borrow_mut();
            windows.retain(|window| window.strong_count() > 0);
            windows.push(Rc::downgrade(&inner));
        });
        Self(inner)
    }

    pub fn window(&self) -> Option<gtk::ApplicationWindow> {
        self.0.window.upgrade()
    }

    /// Original `_reactivate_voice` (SIGUSR1): restart voice input of the
    /// active widget.
    pub fn reactivate_voice(&self) {
        let Some(entry) = self.0.active() else {
            return;
        };
        if let Some(widget) = entry.python_instance().as_ref() {
            if let Some(python) = &self.0.services.python {
                python.reactivate_voice(widget);
            }
            return;
        }
        if let Source::Native(native) = &entry.source
            && let LoadState::Loaded(Instance::Native(widget)) = &*entry.state.borrow()
        {
            native.reactivate_voice(widget);
        }
    }

    pub fn set_compact_mode(&self, compact: bool, animated: bool) {
        self.0.set_compact_mode(compact, animated);
    }

    pub fn is_compact(&self) -> bool {
        self.0.compact_mode.get()
    }

    pub fn toggle_edit_mode(&self) {
        self.0.on_edit_toggle_clicked();
    }

    pub fn is_editing(&self) -> bool {
        self.0.edit_mode.get()
    }

    /// Select the widget whose sidebar row is at `index` (as a click would).
    pub fn select_index(&self, index: usize) {
        let entry = self.0.entries.borrow().get(index).cloned();
        if let Some(entry) = entry {
            self.0.ui.widget_list.select_row(Some(&entry.row));
            self.0.on_widget_selected(&entry);
        }
    }

    /// Titles of the sidebar rows in order (tests and diagnostics).
    pub fn sidebar_titles(&self) -> Vec<String> {
        self.0
            .entries
            .borrow()
            .iter()
            .map(|entry| entry.row.title().to_string())
            .collect()
    }

    /// Name of the visible content page.
    pub fn visible_page(&self) -> Option<String> {
        self.0
            .ui
            .widget_stack
            .visible_child_name()
            .map(String::from)
    }

    /// Named widgets for geometry checks; `None` when absent in this mode.
    pub fn part(&self, part: Part) -> Option<gtk::Widget> {
        let ui = &self.0.ui;
        let sidebar = ui.sidebar.as_ref();
        let entries = self.0.entries.borrow();
        Some(match part {
            Part::Root => match (&ui.split_view, self.window()) {
                (Some(split), _) => split.clone().upcast(),
                (None, Some(window)) => window.child()?,
                (None, None) => return None,
            },
            Part::SidebarContainer => sidebar?.container.clone().upcast(),
            Part::SidebarHeader => sidebar?.header.clone().upcast(),
            Part::SidebarTitle => sidebar?.title.clone().upcast(),
            Part::ListContainer => sidebar?.list_container.clone().upcast(),
            Part::CompactScroll => sidebar?.compact_scroll.clone().upcast(),
            Part::BottomBox => sidebar?.bottom_box.clone().upcast(),
            Part::EditButton => sidebar?.edit_button.clone().upcast(),
            Part::CompactButton => sidebar?.compact_button.clone().upcast(),
            Part::Row(index) => entries.get(index)?.row.clone().upcast(),
            Part::CompactIcon(index) => entries.get(index)?.compact_button.clone().upcast(),
            Part::ContentHeader => ui.content_header.clone().upcast(),
            Part::ContentDisplay => ui.content_display.clone().upcast(),
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Part {
    Root,
    SidebarContainer,
    SidebarHeader,
    SidebarTitle,
    ListContainer,
    CompactScroll,
    BottomBox,
    EditButton,
    CompactButton,
    Row(usize),
    CompactIcon(usize),
    ContentHeader,
    ContentDisplay,
}

impl Inner {
    fn window(&self) -> Option<gtk::ApplicationWindow> {
        self.window.upgrade()
    }

    fn widget_args(&self) -> WidgetArgs {
        WidgetArgs {
            hide_sidebar: self.options.hide_sidebar(),
            voice_autostart: self.options.voice_autostart,
            conversation_id: self.options.conversation_id.clone(),
        }
    }

    fn sidebar(&self) -> Option<&sidebar::Sidebar> {
        self.ui.sidebar.as_ref()
    }

    fn active(&self) -> Option<Rc<Entry>> {
        self.active_entry.borrow().as_ref().and_then(Weak::upgrade)
    }

    /// Run `f` after `ms` milliseconds unless the window state is gone.
    fn after(self: &Rc<Self>, ms: u64, f: impl FnOnce(&Rc<Self>) + 'static) -> glib::SourceId {
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(ms), move || {
            if let Some(this) = weak.upgrade() {
                f(&this);
            }
        })
    }

    /// Run `f` from an idle callback unless the window state is gone.
    fn idle(self: &Rc<Self>, f: impl FnOnce(&Rc<Self>) + 'static) {
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(this) = weak.upgrade() {
                f(&this);
            }
        });
    }

    fn connect_window_signals(self: &Rc<Self>, window: &gtk::ApplicationWindow) {
        if let Some(settings) = &self.wm_settings {
            let weak = Rc::downgrade(self);
            settings.connect_changed(Some("button-layout"), move |settings, _| {
                if let Some(this) = weak.upgrade() {
                    this.button_layout
                        .replace(settings.string("button-layout").into());
                    this.apply_button_layout();
                }
            });
        }
        if !self.options.hide_sidebar() {
            let weak = Rc::downgrade(self);
            window.connect_default_width_notify(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_window_width_changed();
                }
            });
        }
        let weak = Rc::downgrade(self);
        window.connect_close_request(move |_| match weak.upgrade() {
            Some(this) => this.on_close_request(),
            None => glib::Propagation::Proceed,
        });
        let weak = Rc::downgrade(self);
        window.connect_destroy(move |_| {
            if let Some(this) = weak.upgrade()
                && let Some(sidebar) = this.sidebar()
            {
                sidebar.popover.unparent();
            }
        });
        self.register_lock_callback();
    }

    /// Give Python widgets the attributes the original window had.
    fn install_python_window(self: &Rc<Self>, window: &gtk::ApplicationWindow) {
        let Some(python) = &self.services.python else {
            return;
        };
        let weak = Rc::downgrade(self);
        let hooks = WindowHooks {
            refresh_widgets: Box::new(move || {
                if let Some(this) = weak.upgrade() {
                    this.refresh_widgets();
                }
            }),
        };
        python.install_window(window.upcast_ref(), self.options.hide_sidebar(), hooks);
    }

    /// Reconfigure the window controls after `button-layout` changed (the
    /// original rebuilt both header bars with the same settings).
    fn apply_button_layout(&self) {
        let layout = self.button_layout.borrow().clone();
        if let Some(sidebar) = self.sidebar() {
            sidebar.apply_button_layout(&layout, self.services.kinexin);
        }
        content::apply_button_layout(
            &self.ui.content_header,
            &layout,
            self.services.kinexin,
            self.options.hide_sidebar(),
        );
    }
}

impl Ui {
    fn build(
        weak: &Weak<Inner>,
        window: &gtk::ApplicationWindow,
        services: &Services,
        options: &WindowOptions,
        button_layout: &str,
    ) -> Self {
        let hide_sidebar = options.hide_sidebar();
        let widget_stack = content::create_stack(hide_sidebar);
        let content_display = adw::Bin::new();
        content_display.set_hexpand(true);
        content_display.set_vexpand(true);
        content_display.set_child(Some(&widget_stack));
        let widget_list = gtk::ListBox::new();
        widget_list.set_selection_mode(gtk::SelectionMode::Single);
        widget_list.add_css_class("navigation-sidebar");
        let list_weak = weak.clone();
        widget_list.connect_row_activated(move |_, row| {
            if let Some(this) = list_weak.upgrade()
                && let Some(entry) = this.entry_for_row(row)
            {
                this.on_widget_selected(&entry);
            }
        });

        let (content_page, content_header) = content::build_content_area(
            &content_display,
            services.kinexin,
            hide_sidebar,
            button_layout,
        );
        let (sidebar, split_view) = if hide_sidebar {
            set_window_content(window, &content_page, services.kinexin);
            window.set_resizable(true);
            (None, None)
        } else {
            let sidebar = sidebar::Sidebar::build(
                weak,
                window,
                &widget_list,
                services.kinexin,
                button_layout,
            );
            let split_view = adw::NavigationSplitView::new();
            split_view.set_min_sidebar_width(f64::from(SIDEBAR_WIDTH));
            split_view.set_max_sidebar_width(f64::from(SIDEBAR_WIDTH));
            split_view.set_sidebar_width_fraction(0.25);
            split_view.set_sidebar(Some(&sidebar.page));
            split_view.set_content(Some(&content_page));
            set_window_content(window, &split_view, services.kinexin);
            (Some(sidebar), Some(split_view))
        };
        Self {
            widget_stack,
            content_display,
            widget_list,
            content_page,
            content_header,
            sidebar,
            split_view,
        }
    }
}

/// Original `_set_window_content`: `set_child` for the plain GTK window used on
/// Plasma/Hyprland, `set_content` for the Adwaita window elsewhere.
fn set_window_content(
    window: &gtk::ApplicationWindow,
    content: &impl IsA<gtk::Widget>,
    kinexin: bool,
) {
    match window.downcast_ref::<adw::ApplicationWindow>() {
        Some(adw_window) if !kinexin => adw_window.set_content(Some(content)),
        _ => window.set_child(Some(content)),
    }
}
