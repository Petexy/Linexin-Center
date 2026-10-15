//! Native widgets: `.so` files exporting the plugin-api v1 entry point.

use crate::host_api::HOST_API;
use gtk::glib::{self, gobject_ffi, translate::FromGlibPtrFull};
use gtk::prelude::*;
use linexin_plugin_api::{
    ABI_VERSION, CloseRequestV1, CompactVoiceContextV1, CreateCompactVoiceV1, CreateContextV1,
    CreateWidgetV1, ENTRY_SYMBOL, EntryV1, Header, HeaderBarWidgetV1, LxbContextV1, LxbCreateV1,
    LxbDestroyV1, LxbFrameV1, LxbPageV1, LxbReactivateVoiceV1, ReactivateCompactVoiceV1,
    ReactivateVoiceV1, WidgetDescriptorV1,
};
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::offset_of;
use std::path::{Path, PathBuf};
use std::ptr::addr_of;

/// Size of a descriptor written before the LineXinBar fields were appended:
/// the smallest one this Center accepts.
pub const DESCRIPTOR_V1_BASE_SIZE: usize = offset_of!(WidgetDescriptorV1, lxb_mark);

/// Size of a descriptor with the LineXinBar fields but not the voice ones.
const DESCRIPTOR_V1_LXB_SIZE: usize = offset_of!(WidgetDescriptorV1, reactivate_voice);
/// The descriptor's size before `lxb_reactivate_voice` was appended (it
/// then ended with the `--compact` window's callbacks).
const DESCRIPTOR_V1_VOICE_SIZE: usize = offset_of!(WidgetDescriptorV1, lxb_reactivate_voice);

pub struct NativeWidget {
    callbacks: Callbacks,
    pub path: PathBuf,
    pub widget_id: String,
    pub name: String,
    pub icon: String,
    /// The shell mark shown beside the name on LineXinBar (may be empty).
    pub lxb_mark: String,
}

/// The descriptor's functions, copied out of it: an older widget's static
/// descriptor is shorter than this Center's `WidgetDescriptorV1`, so no
/// reference to the whole structure is ever formed.
#[derive(Clone, Copy)]
struct Callbacks {
    create: Option<CreateWidgetV1>,
    header_bar_widget: Option<HeaderBarWidgetV1>,
    close_request: Option<CloseRequestV1>,
    lxb: Option<LxbCallbacks>,
    reactivate_voice: Option<ReactivateVoiceV1>,
    compact_voice: Option<(CreateCompactVoiceV1, ReactivateCompactVoiceV1)>,
}

#[derive(Clone, Copy)]
struct LxbCallbacks {
    create: LxbCreateV1,
    page: LxbPageV1,
    destroy: LxbDestroyV1,
    reactivate_voice: Option<LxbReactivateVoiceV1>,
}

/// A widget's LineXinBar page state (from `lxb_create`), destroyed on drop.
pub struct LxbPage {
    state: *mut c_void,
    callbacks: LxbCallbacks,
}

impl LxbPage {
    /// One frame of the page; `LXB_PAGE_STAY` or `LXB_PAGE_LEAVE`.
    pub fn frame(&mut self, frame: &LxbFrameV1) -> u32 {
        // SAFETY: the state from lxb_create; the frame lives for the call.
        unsafe { (self.callbacks.page)(self.state, frame) }
    }

    /// SIGUSR1 while the page is shown: voice input again, if it listens.
    pub fn reactivate_voice(&mut self) {
        if let Some(callback) = self.callbacks.reactivate_voice {
            // SAFETY: the state from lxb_create, between frames.
            unsafe { callback(self.state) }
        }
    }
}

impl Drop for LxbPage {
    fn drop(&mut self) {
        // SAFETY: the state from lxb_create, released exactly once.
        unsafe { (self.callbacks.destroy)(self.state) }
    }
}

impl std::fmt::Debug for NativeWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeWidget")
            .field("path", &self.path)
            .field("widget_id", &self.widget_id)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl NativeWidget {
    /// Open `path`, call its entry point with the host services and validate
    /// the descriptor. The library is never unloaded: its GTypes, signal
    /// handlers and main-loop sources may outlive any widget.
    pub fn load(path: &Path) -> Result<Self, String> {
        // SAFETY: native widgets are trusted code installed by the administrator.
        let library = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;
        let library: &'static libloading::Library = Box::leak(Box::new(library));
        // SAFETY: the symbol has the documented EntryV1 signature.
        let entry = unsafe { library.get::<EntryV1>(ENTRY_SYMBOL) }
            .map_err(|e| format!("missing entry point {}: {e}", entry_name()))?;
        // SAFETY: calling the entry point as documented by the ABI.
        let pointer = unsafe { entry(&HOST_API) };
        // SAFETY: a non-null result points at a static, aligned descriptor.
        let size = unsafe { validate(pointer) }?;
        // SAFETY: every field read below lies within the validated `size`;
        // fields are read through raw places, never through a reference.
        let (strings, callbacks, lxb_mark) = unsafe {
            let strings = [
                addr_of!((*pointer).widget_id).read(),
                addr_of!((*pointer).name).read(),
                addr_of!((*pointer).icon).read(),
            ];
            let mut callbacks = Callbacks {
                create: addr_of!((*pointer).create).read(),
                header_bar_widget: addr_of!((*pointer).header_bar_widget).read(),
                close_request: addr_of!((*pointer).close_request).read(),
                lxb: None,
                reactivate_voice: None,
                compact_voice: None,
            };
            if size >= DESCRIPTOR_V1_VOICE_SIZE {
                callbacks.reactivate_voice = addr_of!((*pointer).reactivate_voice).read();
                if let (Some(create), Some(reactivate)) = (
                    addr_of!((*pointer).create_compact_voice).read(),
                    addr_of!((*pointer).reactivate_compact_voice).read(),
                ) {
                    callbacks.compact_voice = Some((create, reactivate));
                }
            }
            let mut lxb_mark = std::ptr::null();
            if size >= DESCRIPTOR_V1_LXB_SIZE {
                lxb_mark = addr_of!((*pointer).lxb_mark).read();
                if let (Some(create), Some(page), Some(destroy)) = (
                    addr_of!((*pointer).lxb_create).read(),
                    addr_of!((*pointer).lxb_page).read(),
                    addr_of!((*pointer).lxb_destroy).read(),
                ) {
                    let reactivate_voice = if size >= size_of::<WidgetDescriptorV1>() {
                        addr_of!((*pointer).lxb_reactivate_voice).read()
                    } else {
                        None
                    };
                    callbacks.lxb = Some(LxbCallbacks {
                        create,
                        page,
                        destroy,
                        reactivate_voice,
                    });
                }
            }
            (strings, callbacks, lxb_mark)
        };
        Ok(Self {
            // SAFETY: static strings per the ABI (null is rejected).
            widget_id: unsafe { read_string(strings[0], "widget_id") }?,
            name: unsafe { read_string(strings[1], "name") }?,
            icon: unsafe { read_string(strings[2], "icon") }?,
            lxb_mark: if lxb_mark.is_null() {
                String::new()
            } else {
                // SAFETY: as above.
                unsafe { read_string(lxb_mark, "lxb_mark") }?
            },
            callbacks,
            path: path.to_path_buf(),
        })
        .and_then(Self::check_fields)
    }

    /// Whether the widget has a LineXinBar page (it is listed there only then).
    pub fn has_lxb_page(&self) -> bool {
        self.callbacks.lxb.is_some()
    }

    /// Create the widget's LineXinBar page state.
    pub fn create_lxb_page(
        &self,
        hide_sidebar: bool,
        voice_autostart: bool,
        conversation_id: Option<&str>,
    ) -> Result<LxbPage, String> {
        let callbacks = self
            .callbacks
            .lxb
            .ok_or("the widget has no LineXinBar page")?;
        let conversation = conversation_id.and_then(|id| CString::new(id).ok());
        let context = LxbContextV1 {
            abi_version: ABI_VERSION,
            struct_size: size_of::<LxbContextV1>() as u32,
            host: &HOST_API,
            hide_sidebar,
            voice_autostart,
            conversation_id: conversation
                .as_ref()
                .map_or(std::ptr::null(), |id| id.as_ptr()),
        };
        // SAFETY: the context outlives the call.
        let state = unsafe { (callbacks.create)(&context) };
        if state.is_null() {
            return Err("the widget's page could not be created".into());
        }
        Ok(LxbPage { state, callbacks })
    }

    fn check_fields(self) -> Result<Self, String> {
        let valid_id = !self.widget_id.is_empty()
            && self
                .widget_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b));
        if !valid_id {
            return Err(format!(
                "invalid widget_id {:?} (use ASCII letters, digits, '_', '-' and '.')",
                self.widget_id
            ));
        }
        if self.name.trim().is_empty() {
            return Err("empty widget name".into());
        }
        if self.callbacks.create.is_none() {
            return Err("descriptor has no create function".into());
        }
        Ok(self)
    }

    /// Build the widget's content (GTK main thread), with the `--voice` and
    /// `--conversation` options every widget was offered.
    pub fn create(
        &self,
        window: &gtk::Window,
        hide_sidebar: bool,
        voice_autostart: bool,
        conversation_id: Option<&str>,
    ) -> Result<gtk::Widget, String> {
        let create = self
            .callbacks
            .create
            .ok_or("descriptor has no create function")?;
        let conversation = conversation_id.and_then(|id| CString::new(id).ok());
        let context = CreateContextV1 {
            abi_version: ABI_VERSION,
            struct_size: size_of::<CreateContextV1>() as u32,
            host: &HOST_API,
            window: window.as_ptr().cast(),
            hide_sidebar,
            voice_autostart,
            conversation_id: conversation
                .as_ref()
                .map_or(std::ptr::null(), |id| id.as_ptr()),
        };
        // SAFETY: the context outlives the call; the plugin returns a widget or null.
        let pointer = unsafe { create(&context) };
        // SAFETY: the ABI requires null or a GtkWidget with transfer-full ownership.
        unsafe { adopt_widget(pointer) }?.ok_or_else(|| "the widget could not be created".into())
    }

    /// Optional single-widget header bar controls (legacy `get_header_bar_widget`).
    pub fn header_bar_widget(&self, widget: &gtk::Widget) -> Option<gtk::Widget> {
        let callback = self.callbacks.header_bar_widget?;
        // SAFETY: `widget` is the live widget returned by `create` (borrowed).
        let pointer = unsafe { callback(widget.as_ptr().cast()) };
        // SAFETY: as for `create`.
        unsafe { adopt_widget(pointer) }.unwrap_or_else(|error| {
            eprintln!(
                "{}: header bar widget rejected: {error}",
                self.path.display()
            );
            None
        })
    }

    /// SIGUSR1 while the widget is shown (legacy `stt_toggle` re-toggle).
    pub fn reactivate_voice(&self, widget: &gtk::Widget) {
        if let Some(callback) = self.callbacks.reactivate_voice {
            // SAFETY: `widget` is the live widget returned by `create` (borrowed).
            unsafe { callback(widget.as_ptr().cast()) };
        }
    }

    /// Whether the widget has a `--compact` window.
    pub fn has_compact_voice(&self) -> bool {
        self.callbacks.compact_voice.is_some()
    }

    /// The widget's `--compact` window (legacy `CompactVoiceWindow`).
    pub fn create_compact_voice(
        &self,
        application: &gtk::Application,
        voice_autostart: bool,
    ) -> Result<gtk::Window, String> {
        let (create, _) = self
            .callbacks
            .compact_voice
            .ok_or("the widget has no compact voice window")?;
        let context = CompactVoiceContextV1 {
            abi_version: ABI_VERSION,
            struct_size: size_of::<CompactVoiceContextV1>() as u32,
            host: &HOST_API,
            application: application.as_ptr().cast(),
            voice_autostart,
        };
        // SAFETY: the context outlives the call; the plugin returns a window or null.
        let pointer = unsafe { create(&context) };
        if pointer.is_null() {
            return Err("the compact voice window could not be created".into());
        }
        let object = pointer.cast::<gobject_ffi::GObject>();
        // SAFETY: a GObject instance per the ABI.
        let is_window = unsafe {
            gobject_ffi::g_type_check_instance_is_a(object.cast(), gtk::ffi::gtk_window_get_type())
        } != 0;
        if !is_window {
            // SAFETY: still an owned GObject reference.
            unsafe { gobject_ffi::g_object_unref(object) };
            return Err("the compact voice window is not a GtkWindow".into());
        }
        // SAFETY: a GtkWindow with one reference owned by us (sunk if floating).
        unsafe {
            if gobject_ffi::g_object_is_floating(object) != glib::ffi::GFALSE {
                gobject_ffi::g_object_ref_sink(object);
            }
            Ok(gtk::Window::from_glib_full(
                pointer.cast::<gtk::ffi::GtkWindow>(),
            ))
        }
    }

    /// SIGUSR1 while the compact window is shown.
    pub fn reactivate_compact_voice(&self, window: &gtk::Window) {
        if let Some((_, reactivate)) = self.callbacks.compact_voice {
            // SAFETY: the live window returned by create_compact_voice (borrowed).
            unsafe { reactivate(window.as_ptr().cast()) };
        }
    }

    /// Forward the Center's close request (legacy cleanup hooks).
    pub fn close_request(&self, widget: &gtk::Widget) {
        if let Some(callback) = self.callbacks.close_request {
            // SAFETY: `widget` is the live widget returned by `create` (borrowed).
            unsafe { callback(widget.as_ptr().cast()) };
        }
    }
}

fn entry_name() -> &'static str {
    CStr::from_bytes_with_nul(ENTRY_SYMBOL)
        .ok()
        .and_then(|name| name.to_str().ok())
        .unwrap_or("linexin_widget_v1")
}

/// Check the header before reading anything else of the descriptor, and
/// answer how many bytes of it may be read.
///
/// # Safety
/// `pointer` must be null or point at a readable, aligned, static descriptor
/// prefix of at least [`Header`] size.
unsafe fn validate(pointer: *const WidgetDescriptorV1) -> Result<usize, String> {
    if pointer.is_null() {
        return Err("entry point returned a null descriptor".into());
    }
    // SAFETY: guaranteed by the caller.
    let header = unsafe { *pointer.cast::<Header>() };
    if header.abi_version != ABI_VERSION {
        return Err(format!(
            "unsupported ABI version {} (this Center supports {ABI_VERSION})",
            header.abi_version
        ));
    }
    let required = DESCRIPTOR_V1_BASE_SIZE;
    if (header.struct_size as usize) < required {
        return Err(format!(
            "descriptor is too small ({} bytes, at least {required} required)",
            header.struct_size
        ));
    }
    Ok(header.struct_size as usize)
}

/// # Safety
/// `pointer` must be null or a NUL-terminated static string.
unsafe fn read_string(pointer: *const c_char, field: &str) -> Result<String, String> {
    if pointer.is_null() {
        return Err(format!("descriptor field {field} is null"));
    }
    // SAFETY: guaranteed by the caller.
    unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| format!("descriptor field {field} is not UTF-8"))
}

/// Take ownership of a widget returned across the ABI. `Ok(None)` for null.
///
/// # Safety
/// `pointer` must be null or a GObject instance carrying a reference we own.
unsafe fn adopt_widget(pointer: *mut c_void) -> Result<Option<gtk::Widget>, String> {
    if pointer.is_null() {
        return Ok(None);
    }
    let object = pointer.cast::<gobject_ffi::GObject>();
    // SAFETY: a GObject instance per the caller's guarantee.
    let is_widget = unsafe {
        gobject_ffi::g_type_check_instance_is_a(object.cast(), gtk::ffi::gtk_widget_get_type())
    } != 0;
    if !is_widget {
        // SAFETY: still an owned GObject reference.
        unsafe { gobject_ffi::g_object_unref(object) };
        return Err("returned object is not a GtkWidget".into());
    }
    // A floating reference (e.g. straight from gtk_label_new) becomes ours.
    // SAFETY: valid GObject.
    unsafe {
        if gobject_ffi::g_object_is_floating(object) != glib::ffi::GFALSE {
            gobject_ffi::g_object_ref_sink(object);
        }
    }
    // SAFETY: a GtkWidget with one reference owned by us.
    let widget = unsafe { gtk::Widget::from_glib_full(pointer.cast::<gtk::ffi::GtkWidget>()) };
    if let Some(window) = widget.downcast_ref::<gtk::Window>() {
        window.destroy();
        return Err("returned a window instead of embeddable content".into());
    }
    if widget.parent().is_some() {
        return Err("returned a widget that already has a parent".into());
    }
    Ok(Some(widget))
}
