//! Sidebar entries: one per widget (original `widget_metadata` + sidebar row +
//! compact icon button), and the original icon loading rules.

use super::{ICON_SIZE, Inner};
use crate::config;
use crate::native::NativeWidget;
use adw::prelude::*;
use gtk::{gdk, gdk_pixbuf};
use linexin_localization::global;
use linexin_python_bridge::LoadedWidget;
use std::cell::{Cell, Ref, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

pub(crate) enum Source {
    Native(Rc<NativeWidget>),
    /// `class_name` of the scanned class, when the file was scanned.
    Python {
        class_name: Option<String>,
    },
    /// A `.so` whose descriptor was rejected (the reason is in [`LoadState::Failed`]).
    Broken,
}

pub(crate) enum Instance {
    Native(gtk::Widget),
    Python(LoadedWidget),
}

impl Instance {
    pub fn widget(&self) -> &gtk::Widget {
        match self {
            Self::Native(widget) => widget,
            Self::Python(loaded) => &loaded.widget,
        }
    }
}

pub(crate) enum LoadState {
    /// Registered from metadata; not created/imported yet (original `_deferred_widgets`).
    Deferred,
    /// Python module being imported on a worker thread (`_bg_load_in_progress`).
    Importing,
    Loaded(Instance),
    Failed(String),
}

pub(crate) struct Metadata {
    /// Untranslated name (original `widget_name` key).
    pub widget_name: String,
    /// Translation domain.
    pub widget_id: String,
    pub icon: Option<String>,
}

pub(crate) struct Entry {
    pub file_name: String,
    pub path: PathBuf,
    pub source: Source,
    pub meta: RefCell<Metadata>,
    /// Translated sidebar title (original `row.display_name`).
    pub display_name: String,
    pub hidden: Cell<bool>,
    pub row: adw::ActionRow,
    pub compact_button: gtk::Button,
    pub state: RefCell<LoadState>,
    pub edit_controls: RefCell<Option<gtk::Box>>,
    pub dnd_controllers: RefCell<Vec<gtk::EventController>>,
}

impl Entry {
    pub fn is_loaded(&self) -> bool {
        matches!(*self.state.borrow(), LoadState::Loaded(_))
    }

    pub fn python_instance(&self) -> Option<Ref<'_, LoadedWidget>> {
        Ref::filter_map(self.state.borrow(), |state| match state {
            LoadState::Loaded(Instance::Python(loaded)) => Some(loaded),
            _ => None,
        })
        .ok()
    }

    pub fn widget_name(&self) -> String {
        self.meta.borrow().widget_name.clone()
    }

    pub fn widget_id(&self) -> String {
        self.meta.borrow().widget_id.clone()
    }
}

impl Inner {
    /// Original `_add_widget_to_sidebar` (plus the metadata bookkeeping of
    /// `_register_deferred_widget` / `_register_widget`).
    pub(super) fn add_entry(
        self: &Rc<Self>,
        file_name: &str,
        path: PathBuf,
        source: Source,
        meta: Metadata,
        state: LoadState,
    ) -> Rc<Entry> {
        let display_name = global().get_text(&meta.widget_name, &meta.widget_id);
        let hidden = self.is_hidden_file(file_name);
        let row = adw::ActionRow::new();
        row.set_title(&display_name);
        row.set_activatable(true);
        let icon = self.load_widget_icon(meta.icon.as_deref(), &source);
        row.add_prefix(&icon);
        if hidden && !self.edit_mode.get() {
            row.set_visible(false);
        }
        self.ui.widget_list.append(&row);

        let compact_button = gtk::Button::new();
        compact_button.add_css_class("flat");
        compact_button.add_css_class("compact-icon-btn");
        compact_button.set_halign(gtk::Align::Center);
        compact_button.set_tooltip_text(Some(&display_name));
        compact_button.set_child(Some(&self.load_widget_icon(meta.icon.as_deref(), &source)));
        if let Some(sidebar) = self.sidebar() {
            sidebar.compact_strip.append(&compact_button);
        }
        if hidden && !self.edit_mode.get() {
            compact_button.set_visible(false);
        }

        let entry = Rc::new(Entry {
            file_name: file_name.to_owned(),
            path,
            source,
            meta: RefCell::new(meta),
            display_name,
            hidden: Cell::new(hidden),
            row,
            compact_button,
            state: RefCell::new(state),
            edit_controls: RefCell::default(),
            dnd_controllers: RefCell::default(),
        });
        let weak_self = Rc::downgrade(self);
        let weak_entry = Rc::downgrade(&entry);
        entry.compact_button.connect_clicked(move |_| {
            if let (Some(this), Some(entry)) = (weak_self.upgrade(), weak_entry.upgrade()) {
                this.on_compact_icon_clicked(&entry);
            }
        });
        self.entries.borrow_mut().push(entry.clone());
        entry
    }

    pub(super) fn entry_for_row(&self, row: &gtk::ListBoxRow) -> Option<Rc<Entry>> {
        self.entries
            .borrow()
            .iter()
            .find(|entry| entry.row.upcast_ref::<gtk::ListBoxRow>() == row)
            .cloned()
    }

    pub(super) fn index_of(&self, entry: &Entry) -> Option<usize> {
        self.entries
            .borrow()
            .iter()
            .position(|candidate| std::ptr::eq(candidate.as_ref(), entry))
    }

    pub(super) fn find_entry(&self, entry: &Option<Weak<Entry>>) -> Option<Rc<Entry>> {
        entry.as_ref().and_then(Weak::upgrade)
    }

    /// Hidden state by file name; a `<stem>.py` hidden by the Python package
    /// also hides the native `<stem>.so` that replaced it.
    fn is_hidden_file(&self, file_name: &str) -> bool {
        let hidden = self.hidden_widgets.borrow();
        config::hidden_names(file_name).any(|name| hidden.contains(&name))
    }

    /// Original `_load_widget_icon`. Native widgets may also name a themed icon.
    fn load_widget_icon(&self, icon: Option<&str>, source: &Source) -> gtk::Image {
        let icon = icon.unwrap_or_default();
        match self.services.paths.icon_file(icon) {
            Some(path) => image_from_file(&path).unwrap_or_else(|error| {
                println!("Error loading icon from {}: {error}", path.display());
                fallback_icon()
            }),
            None if matches!(source, Source::Native(_)) && is_icon_name(icon) => {
                let image = gtk::Image::from_icon_name(icon);
                image.set_pixel_size(ICON_SIZE);
                image
            }
            None => fallback_icon(),
        }
    }
}

fn is_icon_name(icon: &str) -> bool {
    !icon.is_empty()
        && !icon.contains('/')
        && gtk::gdk::Display::default()
            .is_some_and(|display| gtk::IconTheme::for_display(&display).has_icon(icon))
}

fn fallback_icon() -> gtk::Image {
    let image = gtk::Image::from_icon_name("application-x-addon-symbolic");
    image.set_pixel_size(ICON_SIZE);
    image
}

fn image_from_file(path: &std::path::Path) -> Result<gtk::Image, String> {
    let is_svg = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    let image = if is_svg {
        let image = gtk::Image::new();
        image.set_from_file(Some(path));
        image
    } else {
        let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_size(path, ICON_SIZE, ICON_SIZE)
            .map_err(|error| error.to_string())?;
        #[allow(deprecated)] // Same call as the original; GTK keeps it working.
        let texture = gdk::Texture::for_pixbuf(&pixbuf);
        gtk::Image::from_paintable(Some(&texture))
    };
    image.set_pixel_size(ICON_SIZE);
    Ok(image)
}

/// Original `_scan_widget_metadata` fallback id.
pub(crate) fn default_widget_id(widget_name: &str) -> String {
    widget_name.to_lowercase().replace(' ', "_")
}
