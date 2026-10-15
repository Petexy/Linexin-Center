//! Version-one ABI for native Linexin Center widgets (`.so` files).
//!
//! A native widget is a `cdylib` installed next to the legacy Python widgets in
//! `/usr/share/linexin/widgets/`. It exports [`ENTRY_SYMBOL`] with the
//! [`EntryV1`] signature. Only C-compatible values cross this boundary; Rust
//! strings, trait objects, gtk-rs wrappers and Rust-allocated containers never do.
//!
//! The host reads the [`Header`] prefix of every structure it receives, checks
//! [`ABI_VERSION`] and `struct_size`, and only then reads the rest. Plugins must
//! do the same with structures the host passes to them. Fields may be appended
//! to a structure in later revisions of version one; readers must check
//! `struct_size` before touching a field beyond the size they were built with.
//!
//! Libraries stay loaded until the process exits (GTK signal handlers, GTypes and
//! pending main-loop sources may still point into their code).
//!
//! # Translations
//!
//! Native widgets use the same dictionaries as Python widgets:
//! `<widgets dir>/localization/<lang>/<widget_id>_dictionary.py` containing
//! `translations = {"English": "Translated", ...}`. The host loads them and
//! exposes them through [`HostApiV1::translate`], including the original
//! `{}` placeholder matching. The sidebar title is translated by the host with
//! the descriptor's `widget_id` as the dictionary domain.
//!
//! # Writing a widget
//!
//! ```ignore
//! use linexin_plugin_api as api;
//!
//! static WIDGET: api::WidgetDescriptorV1 = api::WidgetDescriptorV1::new(
//!     c"system_information",            // widget_id = translation domain
//!     c"System Information",            // untranslated widgetname
//!     c"/usr/share/icons/computer-linexin.svg",
//!     Some(create),
//! );
//! api::export_widget!(WIDGET);
//!
//! unsafe extern "C" fn create(context: *const api::CreateContextV1) -> *mut std::ffi::c_void {
//!     // validate `context` with api::CreateContextV1::read, call gtk::init(),
//!     // build the widget, return it with into_glib_ptr() (transfer full).
//!     std::ptr::null_mut()
//! }
//! ```
//! Inside the widget use [`tr`] (or [`Host::tr`]) for every user-visible string.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::atomic::{AtomicPtr, Ordering};

/// The Center's stylesheet, which its GTK window applies around every widget
/// page: for a harness that shows a widget alone as the Center would, without
/// having to find the Center's sources.
pub const CENTER_STYLE_CSS: &str = include_str!("../../center/src/window/style.css");

/// The only ABI version this crate describes.
pub const ABI_VERSION: u32 = 1;
/// Exported symbol name (NUL terminated) of a native widget's [`EntryV1`].
pub const ENTRY_SYMBOL: &[u8] = b"linexin_widget_v1\0";

/// Readable prefix shared by every versioned structure.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub abi_version: u32,
    pub struct_size: u32,
}

impl Header {
    /// `true` when `pointer` is non-null, reports [`ABI_VERSION`] and is at least
    /// `minimum_size` bytes long.
    ///
    /// # Safety
    /// A non-null `pointer` must point to at least a readable, aligned [`Header`].
    pub unsafe fn is_compatible(pointer: *const c_void, minimum_size: usize) -> bool {
        if pointer.is_null() {
            return false;
        }
        // SAFETY: guaranteed by the caller.
        let header = unsafe { &*pointer.cast::<Header>() };
        header.abi_version == ABI_VERSION && header.struct_size as usize >= minimum_size
    }
}

/// Services provided by the host. The pointer stays valid for the whole process.
///
/// Every function is safe to call from any thread unless documented as
/// "GTK main thread only". Strings passed in are borrowed, NUL-terminated UTF-8.
#[repr(C)]
pub struct HostApiV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    /// Active language code such as `pl_PL` (after `--lang` override and locale
    /// detection). Static string, never null.
    pub language: unsafe extern "C" fn() -> *const c_char,
    /// Translate `text` with the dictionaries of `domain` (normally the widget's
    /// `widget_id`), exactly like the legacy injected `_()`: exact match in the
    /// active language, then `en_US`, then `{}` placeholder patterns, otherwise
    /// `text` unchanged. Returns a new string that must be released with
    /// [`HostApiV1::free_string`]; null only when an argument is null.
    pub translate: unsafe extern "C" fn(domain: *const c_char, text: *const c_char) -> *mut c_char,
    /// Release a string returned by [`HostApiV1::translate`]. Accepts null.
    pub free_string: unsafe extern "C" fn(string: *mut c_char),
    /// Recursively translate a widget tree in place (labels, buttons, entries'
    /// placeholders, Adw rows/groups/pages/status pages, window titles, and the
    /// heading/body of Adw message/alert dialogs). Equivalent of the legacy
    /// `translate_dialog`/`translate_window`. GTK main thread only.
    pub translate_widget: unsafe extern "C" fn(domain: *const c_char, widget: *mut c_void),
    /// Lock the Center while a critical command runs: the sidebar becomes
    /// insensitive, window controls are hidden and closing is blocked. Calls
    /// are counted; every `lock_ui` needs one `unlock_ui`.
    pub lock_ui: unsafe extern "C" fn(),
    /// Undo one [`HostApiV1::lock_ui`].
    pub unlock_ui: unsafe extern "C" fn(),
    /// Check a password with `sudo -S -v`. Blocks; do not call on the GTK thread.
    pub sudo_validate_password: unsafe extern "C" fn(password: *const c_char) -> bool,
    /// Store a validated password in the shared credential helper (the same one
    /// the legacy Python widgets use as `sudo_manager`).
    pub sudo_set_password: unsafe extern "C" fn(password: *const c_char),
    /// Whether a password is currently stored.
    pub sudo_has_password: unsafe extern "C" fn() -> bool,
    /// Drop the stored password and invalidate sudo's cache (`sudo -k`).
    pub sudo_forget_password: unsafe extern "C" fn(),
    /// Path of the wrapper script: running `wrapper cmd args...` executes
    /// `sudo -A cmd args...` with the stored password supplied by askpass.
    /// Suitable for `PACMAN_AUTH`-style variables. Static string, never null.
    pub sudo_wrapper_path: unsafe extern "C" fn() -> *const c_char,
    /// Allow the stored password to be supplied any number of times until
    /// [`HostApiV1::sudo_stop_session`] (long multi-step operations).
    pub sudo_start_session: unsafe extern "C" fn(),
    /// Close the session opened by [`HostApiV1::sudo_start_session`].
    pub sudo_stop_session: unsafe extern "C" fn(),
    /// Run a NULL-terminated argv through the wrapper with a single password
    /// feed while the UI is locked. Blocks until the command exits; returns its
    /// exit code, or -1 when it could not be started, was killed, or no password
    /// is stored. Do not call on the GTK thread.
    pub sudo_run_privileged: unsafe extern "C" fn(argv: *const *const c_char) -> c_int,
    /// Ask the Center to pick up widgets installed since it started (the
    /// legacy `window.refresh_widgets()`), e.g. after installing a package
    /// that ships one. Any thread; the rescan happens on the Center's own loop.
    pub refresh_widgets: unsafe extern "C" fn(),
    /// Path of the askpass script behind the wrapper (it prints the stored
    /// password once per feed), for tools that call `sudo` themselves and
    /// read `SUDO_ASKPASS` (the legacy `sudo_manager.askpass_script`).
    /// Static string, never null.
    pub sudo_askpass_path: unsafe extern "C" fn() -> *const c_char,
    /// Whether the Center window `window` (the `GtkWindow*` of
    /// [`CreateContextV1::window`]) is in its compact mode, the sidebar shrunk
    /// to icons (the legacy `window._compact_mode`). `false` for any other
    /// window. GTK main thread only.
    pub window_compact_mode: unsafe extern "C" fn(window: *mut c_void) -> bool,
}

/// Context passed to [`WidgetDescriptorV1::create`]. Valid only during that call.
#[repr(C)]
pub struct CreateContextV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    /// Same pointer the entry point received.
    pub host: *const HostApiV1,
    /// Borrowed `GtkWindow*` of the Center (never null). Take a GObject
    /// reference if you keep it, avoiding reference cycles with your content.
    pub window: *mut c_void,
    /// `true` in single-widget mode (`linexin-center -w <widget>`), when the
    /// Center shows only this widget without a sidebar.
    pub hide_sidebar: bool,
    /// `--voice`: start voice input at once (legacy `voice_autostart`).
    pub voice_autostart: bool,
    /// `--conversation ID` (legacy `conversation_id`): borrowed, or null.
    pub conversation_id: *const c_char,
}

impl CreateContextV1 {
    /// Validate and borrow a context received by a `create` callback.
    ///
    /// # Safety
    /// `pointer` must be the value the host passed to `create`.
    pub unsafe fn read<'a>(pointer: *const CreateContextV1) -> Option<&'a CreateContextV1> {
        // SAFETY: the host passes a readable, aligned header (or null).
        unsafe { Header::is_compatible(pointer.cast(), size_of::<CreateContextV1>()) }
            .then(|| unsafe { &*pointer })
    }

    /// [`CreateContextV1::conversation_id`], copied.
    pub fn conversation_id(&self) -> Option<String> {
        (!self.conversation_id.is_null()).then(|| {
            // SAFETY: a NUL-terminated string the host keeps for the call.
            unsafe { CStr::from_ptr(self.conversation_id) }
                .to_string_lossy()
                .into_owned()
        })
    }
}

/// Create the widget's content on the GTK main thread.
///
/// Return a non-null, unparented `GtkWidget*` carrying one owned
/// **non-floating** reference (transfer full; gtk-rs `into_glib_ptr()` does this),
/// or null on failure. Must not return a `GtkWindow`, create an application or
/// run a main loop. Call `gtk::init()` first: a `cdylib` has its own gtk-rs
/// "initialized" flag even though GTK itself is already running. No panic may
/// unwind out of this function or any later GTK callback.
pub type CreateWidgetV1 = unsafe extern "C" fn(context: *const CreateContextV1) -> *mut c_void;

/// Optional: return extra controls for the header bar in single-widget mode
/// (legacy `get_header_bar_widget`). Receives the widget returned by `create`
/// (borrowed). Return an unparented widget with transfer-full ownership, or null.
pub type HeaderBarWidgetV1 = unsafe extern "C" fn(widget: *mut c_void) -> *mut c_void;

/// Optional: the Center's window received a close request (legacy cleanup
/// hooks). Receives the widget returned by `create` (borrowed). GTK thread.
pub type CloseRequestV1 = unsafe extern "C" fn(widget: *mut c_void);

/// Optional: SIGUSR1 reached the Center while this widget was shown (the
/// legacy handler toggled `stt_toggle` off and on): restart voice input.
/// Receives the widget returned by `create` (borrowed). GTK thread.
pub type ReactivateVoiceV1 = unsafe extern "C" fn(widget: *mut c_void);

/// Context passed to [`WidgetDescriptorV1::create_compact_voice`]. Valid only
/// during that call.
#[repr(C)]
pub struct CompactVoiceContextV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub host: *const HostApiV1,
    /// Borrowed `GtkApplication*` the window belongs to.
    pub application: *mut c_void,
    /// `--voice` was given too.
    pub voice_autostart: bool,
}

impl CompactVoiceContextV1 {
    /// Validate and borrow a context received by `create_compact_voice`.
    ///
    /// # Safety
    /// `pointer` must be the value the host passed to `create_compact_voice`.
    pub unsafe fn read<'a>(
        pointer: *const CompactVoiceContextV1,
    ) -> Option<&'a CompactVoiceContextV1> {
        // SAFETY: the host passes a readable, aligned header (or null).
        unsafe { Header::is_compatible(pointer.cast(), size_of::<CompactVoiceContextV1>()) }
            .then(|| unsafe { &*pointer })
    }
}

/// Optional: `linexin-center -w <widget> --compact` (legacy
/// `CompactVoiceWindow`): return the widget's own small window, a
/// `GtkWindow*` of `application` with one owned reference (transfer full), not
/// yet presented; or null. GTK main thread.
pub type CreateCompactVoiceV1 =
    unsafe extern "C" fn(context: *const CompactVoiceContextV1) -> *mut c_void;

/// Optional: SIGUSR1 reached the Center while the compact window returned by
/// [`CreateCompactVoiceV1`] was shown (borrowed): restart voice input there.
pub type ReactivateCompactVoiceV1 = unsafe extern "C" fn(window: *mut c_void);

/// Context passed to [`WidgetDescriptorV1::lxb_create`]. Valid only during that call.
///
/// On a LineXinBar session (`XDG_CURRENT_DESKTOP` names `LineXinBar`) the Center
/// does not start GTK at all: it is an lxb-toolkit application, and a widget
/// takes part with an lxb page instead of a GTK widget. Widgets without one
/// are not listed there.
#[repr(C)]
pub struct LxbContextV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    /// Same pointer the entry point received.
    pub host: *const HostApiV1,
    /// `true` in single-widget mode, when the page is the whole window.
    pub hide_sidebar: bool,
    /// `--voice` (or `--compact`, which has no window of its own on
    /// LineXinBar): start voice input at once. Appended; see
    /// [`LxbContextV1::voice_autostart`].
    pub voice_autostart: bool,
    /// `--conversation ID`: borrowed, or null. Appended; see
    /// [`LxbContextV1::conversation_id`].
    pub conversation_id: *const c_char,
}

impl LxbContextV1 {
    /// Where the fields appended after `hide_sidebar` begin: a host older
    /// than them passes a smaller context.
    const BASE_SIZE: usize = std::mem::offset_of!(LxbContextV1, voice_autostart);

    /// [`LxbContextV1::voice_autostart`], `false` from an older host.
    pub fn voice_autostart(&self) -> bool {
        self.struct_size as usize >= std::mem::offset_of!(LxbContextV1, conversation_id)
            && self.voice_autostart
    }

    /// [`LxbContextV1::conversation_id`], copied (`None` from an older host).
    pub fn conversation_id(&self) -> Option<String> {
        (self.struct_size as usize >= size_of::<LxbContextV1>() && !self.conversation_id.is_null())
            .then(|| {
                // SAFETY: a NUL-terminated string the host keeps for the call.
                unsafe { CStr::from_ptr(self.conversation_id) }
                    .to_string_lossy()
                    .into_owned()
            })
    }

    /// Validate and borrow a context received by an `lxb_create` callback.
    ///
    /// # Safety
    /// `pointer` must be the value the host passed to `lxb_create`.
    pub unsafe fn read<'a>(pointer: *const LxbContextV1) -> Option<&'a LxbContextV1> {
        // SAFETY: the host passes a readable, aligned header (or null). An
        // older host's context ends after `hide_sidebar`; the accessors of
        // the appended fields check `struct_size`.
        unsafe { Header::is_compatible(pointer.cast(), Self::BASE_SIZE) }
            .then(|| unsafe { &*pointer })
    }
}

/// One frame of a widget's lxb page. Valid only during the
/// [`WidgetDescriptorV1::lxb_page`] call.
#[repr(C)]
pub struct LxbFrameV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    /// Borrowed `lxb_page*` of the process's `liblxb_app.so` (see `lxb_app.h`).
    /// The widget draws with the `lxb_page_*`/`lxb_draw_*` functions of that
    /// same library, loaded with `dlopen("liblxb_app.so")` (never linked, so
    /// the widget still loads where LineXinBar is not installed).
    pub page: *mut c_void,
    /// Where the page goes, in window pixels: x, y, width, height. The flow
    /// cursor is already placed here (`lxb_page_set_cursor`).
    pub rect: [f32; 4],
    /// Whether the page holds the light (the user moved into it). When false
    /// it is drawn beside the Center's column as a preview and gets no input.
    pub focused: bool,
    /// This frame's actions for the page, as `lxb_action` indices in the
    /// order they happened (the Center reads them with `lxb_page_action`; a
    /// widget must not call that itself). Empty unless `focused`.
    pub actions: *const c_int,
    pub action_count: usize,
}

impl LxbFrameV1 {
    /// Validate and borrow a frame received by an `lxb_page` callback.
    ///
    /// # Safety
    /// `pointer` must be the value the host passed to `lxb_page`.
    pub unsafe fn read<'a>(pointer: *const LxbFrameV1) -> Option<&'a LxbFrameV1> {
        // SAFETY: the host passes a readable, aligned header (or null).
        unsafe { Header::is_compatible(pointer.cast(), size_of::<LxbFrameV1>()) }
            .then(|| unsafe { &*pointer })
    }

    /// This frame's actions (`lxb_action` indices).
    pub fn actions(&self) -> &[c_int] {
        if self.actions.is_null() || self.action_count == 0 {
            return &[];
        }
        // SAFETY: the host passes `action_count` readable values.
        unsafe { std::slice::from_raw_parts(self.actions, self.action_count) }
    }
}

/// [`WidgetDescriptorV1::lxb_page`] result: the page keeps the light.
pub const LXB_PAGE_STAY: u32 = 0;
/// [`WidgetDescriptorV1::lxb_page`] result: give the light back to the
/// Center's column — what Back (or Left at the page's first column) means
/// when the page has nothing of its own left to close.
pub const LXB_PAGE_LEAVE: u32 = 1;

/// Create the widget's lxb state on the Center's main thread (no GTK is
/// running). Return an opaque non-null pointer, or null on failure.
pub type LxbCreateV1 = unsafe extern "C" fn(context: *const LxbContextV1) -> *mut c_void;

/// Draw one frame of the page and act on `frame.actions`. Returns
/// [`LXB_PAGE_STAY`] or [`LXB_PAGE_LEAVE`]. Called every frame while the page
/// is on screen; anything slow belongs on a thread of the widget's own, with
/// `lxb_page_redraw_within` asking for the frame that shows its result.
pub type LxbPageV1 = unsafe extern "C" fn(state: *mut c_void, frame: *const LxbFrameV1) -> u32;

/// Release the state returned by [`LxbCreateV1`].
pub type LxbDestroyV1 = unsafe extern "C" fn(state: *mut c_void);

/// Optional: SIGUSR1 reached the Center (on LineXinBar) while this widget's
/// page was shown — the wake-word daemon asking for voice input again.
/// Receives the state returned by [`LxbCreateV1`]; called between frames.
pub type LxbReactivateVoiceV1 = unsafe extern "C" fn(state: *mut c_void);

/// Immutable descriptor returned by the entry point; must live as long as the
/// library. All strings are static NUL-terminated UTF-8.
///
/// Fields after `close_request` were appended later in version one: the host
/// reads them only when `struct_size` covers them.
#[repr(C)]
pub struct WidgetDescriptorV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    /// Stable identifier and translation domain, e.g. `system_information`
    /// (legacy `widget_id`). ASCII letters, digits, `_`, `-` and `.` only.
    pub widget_id: *const c_char,
    /// Untranslated display name (legacy `widgetname`), e.g. `System Information`.
    pub name: *const c_char,
    /// Absolute path of an SVG/PNG icon, or a themed icon name; may be empty.
    pub icon: *const c_char,
    pub create: Option<CreateWidgetV1>,
    pub header_bar_widget: Option<HeaderBarWidgetV1>,
    pub close_request: Option<CloseRequestV1>,
    /// The shell mark (one of the 108 `lxb_glyph_name`s, e.g. `setting-info`)
    /// beside the widget's name in the LineXinBar column; may be empty.
    pub lxb_mark: *const c_char,
    /// The LineXinBar page. All three are present, or the widget has no
    /// LineXinBar page and is not listed on a LineXinBar session.
    pub lxb_create: Option<LxbCreateV1>,
    pub lxb_page: Option<LxbPageV1>,
    pub lxb_destroy: Option<LxbDestroyV1>,
    /// SIGUSR1 while the widget is shown; see [`ReactivateVoiceV1`].
    pub reactivate_voice: Option<ReactivateVoiceV1>,
    /// The widget's `--compact` window; see [`CreateCompactVoiceV1`].
    pub create_compact_voice: Option<CreateCompactVoiceV1>,
    pub reactivate_compact_voice: Option<ReactivateCompactVoiceV1>,
    /// SIGUSR1 while the widget's LineXinBar page is shown; see
    /// [`LxbReactivateVoiceV1`].
    pub lxb_reactivate_voice: Option<LxbReactivateVoiceV1>,
}

// The descriptor is immutable; its raw pointers refer to static data.
unsafe impl Sync for WidgetDescriptorV1 {}

impl WidgetDescriptorV1 {
    /// Descriptor without the optional callbacks.
    pub const fn new(
        widget_id: &'static CStr,
        name: &'static CStr,
        icon: &'static CStr,
        create: Option<CreateWidgetV1>,
    ) -> Self {
        Self {
            abi_version: ABI_VERSION,
            struct_size: size_of::<Self>() as u32,
            widget_id: widget_id.as_ptr(),
            name: name.as_ptr(),
            icon: icon.as_ptr(),
            create,
            header_bar_widget: None,
            close_request: None,
            lxb_mark: c"".as_ptr(),
            lxb_create: None,
            lxb_page: None,
            lxb_destroy: None,
            reactivate_voice: None,
            create_compact_voice: None,
            reactivate_compact_voice: None,
            lxb_reactivate_voice: None,
        }
    }

    /// Restart voice input on the LineXinBar page on SIGUSR1 (see
    /// [`LxbReactivateVoiceV1`]).
    pub const fn with_lxb_reactivate_voice(mut self, callback: LxbReactivateVoiceV1) -> Self {
        self.lxb_reactivate_voice = Some(callback);
        self
    }

    /// Restart voice input on SIGUSR1 (see [`ReactivateVoiceV1`]).
    pub const fn with_reactivate_voice(mut self, callback: ReactivateVoiceV1) -> Self {
        self.reactivate_voice = Some(callback);
        self
    }

    /// Give the widget a `--compact` window (see [`CreateCompactVoiceV1`]).
    pub const fn with_compact_voice(
        mut self,
        create: CreateCompactVoiceV1,
        reactivate: ReactivateCompactVoiceV1,
    ) -> Self {
        self.create_compact_voice = Some(create);
        self.reactivate_compact_voice = Some(reactivate);
        self
    }

    /// Give the widget a LineXinBar page (see [`LxbFrameV1`]).
    pub const fn with_lxb_page(
        mut self,
        mark: &'static CStr,
        create: LxbCreateV1,
        page: LxbPageV1,
        destroy: LxbDestroyV1,
    ) -> Self {
        self.lxb_mark = mark.as_ptr();
        self.lxb_create = Some(create);
        self.lxb_page = Some(page);
        self.lxb_destroy = Some(destroy);
        self
    }

    pub const fn with_header_bar_widget(mut self, callback: HeaderBarWidgetV1) -> Self {
        self.header_bar_widget = Some(callback);
        self
    }

    pub const fn with_close_request(mut self, callback: CloseRequestV1) -> Self {
        self.close_request = Some(callback);
        self
    }
}

/// Entry point exported as [`ENTRY_SYMBOL`]. Receives the host services
/// (validate with [`Header::is_compatible`]) and returns the static descriptor,
/// or null on failure. Must not construct GTK objects or unwind.
pub type EntryV1 = unsafe extern "C" fn(host: *const HostApiV1) -> *const WidgetDescriptorV1;

static HOST: AtomicPtr<HostApiV1> = AtomicPtr::new(std::ptr::null_mut());

/// Remember the host services for [`host`] and [`tr`]. Called by
/// [`export_widget!`]; returns `false` when `api` is incompatible.
///
/// # Safety
/// `api` must be null or the pointer the host passed to the entry point.
pub unsafe fn install_host(api: *const HostApiV1) -> bool {
    // SAFETY: guaranteed by the caller.
    if unsafe { Header::is_compatible(api.cast(), size_of::<HostApiV1>()) } {
        HOST.store(api.cast_mut(), Ordering::Release);
        true
    } else {
        false
    }
}

/// Host services installed by [`export_widget!`], if any.
pub fn host() -> Option<Host> {
    let pointer = HOST.load(Ordering::Acquire);
    // SAFETY: only validated, process-lifetime pointers are stored.
    (!pointer.is_null()).then(|| Host(unsafe { &*pointer }))
}

/// Translate `text` in `domain`; returns `text` unchanged without a host.
pub fn tr(domain: &str, text: &str) -> String {
    match host() {
        Some(host) => host.tr(domain, text),
        None => text.to_owned(),
    }
}

/// Safe wrapper around [`HostApiV1`].
#[derive(Clone, Copy)]
pub struct Host(&'static HostApiV1);

impl Host {
    pub fn api(&self) -> &'static HostApiV1 {
        self.0
    }

    pub fn language(&self) -> String {
        // SAFETY: the host returns a static NUL-terminated string.
        unsafe { CStr::from_ptr((self.0.language)()) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn tr(&self, domain: &str, text: &str) -> String {
        let (Ok(c_domain), Ok(c_text)) = (CString::new(domain), CString::new(text)) else {
            return text.to_owned();
        };
        // SAFETY: valid NUL-terminated arguments; result released below.
        let result = unsafe { (self.0.translate)(c_domain.as_ptr(), c_text.as_ptr()) };
        if result.is_null() {
            return text.to_owned();
        }
        // SAFETY: non-null result is a NUL-terminated string owned by us.
        let translated = unsafe { CStr::from_ptr(result) }
            .to_string_lossy()
            .into_owned();
        unsafe { (self.0.free_string)(result) };
        translated
    }

    /// GTK main thread only. `widget` must be a live `GtkWidget*`.
    ///
    /// # Safety
    /// See [`HostApiV1::translate_widget`].
    pub unsafe fn translate_widget(&self, domain: &str, widget: *mut c_void) {
        if let Ok(c_domain) = CString::new(domain) {
            unsafe { (self.0.translate_widget)(c_domain.as_ptr(), widget) }
        }
    }

    pub fn lock_ui(&self) {
        unsafe { (self.0.lock_ui)() }
    }

    pub fn unlock_ui(&self) {
        unsafe { (self.0.unlock_ui)() }
    }

    pub fn sudo_validate_password(&self, password: &str) -> bool {
        CString::new(password)
            .map(|p| unsafe { (self.0.sudo_validate_password)(p.as_ptr()) })
            .unwrap_or(false)
    }

    pub fn sudo_set_password(&self, password: &str) {
        if let Ok(p) = CString::new(password) {
            unsafe { (self.0.sudo_set_password)(p.as_ptr()) }
        }
    }

    pub fn sudo_has_password(&self) -> bool {
        unsafe { (self.0.sudo_has_password)() }
    }

    pub fn sudo_forget_password(&self) {
        unsafe { (self.0.sudo_forget_password)() }
    }

    pub fn sudo_wrapper_path(&self) -> String {
        unsafe { CStr::from_ptr((self.0.sudo_wrapper_path)()) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn sudo_start_session(&self) {
        unsafe { (self.0.sudo_start_session)() }
    }

    pub fn sudo_stop_session(&self) {
        unsafe { (self.0.sudo_stop_session)() }
    }

    /// Returns the exit code, or -1 (see [`HostApiV1::sudo_run_privileged`]).
    pub fn sudo_run_privileged(&self, argv: &[&str]) -> i32 {
        let Ok(owned) = argv
            .iter()
            .map(|a| CString::new(*a))
            .collect::<Result<Vec<_>, _>>()
        else {
            return -1;
        };
        let mut pointers: Vec<*const c_char> = owned.iter().map(|a| a.as_ptr()).collect();
        pointers.push(std::ptr::null());
        unsafe { (self.0.sudo_run_privileged)(pointers.as_ptr()) }
    }

    /// See [`HostApiV1::refresh_widgets`].
    pub fn refresh_widgets(&self) {
        unsafe { (self.0.refresh_widgets)() }
    }

    pub fn sudo_askpass_path(&self) -> String {
        unsafe { CStr::from_ptr((self.0.sudo_askpass_path)()) }
            .to_string_lossy()
            .into_owned()
    }

    /// See [`HostApiV1::window_compact_mode`]. GTK main thread only.
    // The host only compares the address with its windows', never reads it.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn window_compact_mode(&self, window: *mut c_void) -> bool {
        unsafe { (self.0.window_compact_mode)(window) }
    }
}

/// Export `$descriptor` (a `static WidgetDescriptorV1`) as this library's
/// [`ENTRY_SYMBOL`], installing the host services for [`host`]/[`tr`].
#[macro_export]
macro_rules! export_widget {
    ($descriptor:path) => {
        /// Linexin Center native widget entry point.
        ///
        /// # Safety
        /// Called by the host with its `HostApiV1` pointer.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn linexin_widget_v1(
            host: *const $crate::HostApiV1,
        ) -> *const $crate::WidgetDescriptorV1 {
            ::std::panic::catch_unwind(|| {
                // SAFETY: forwarded from the host.
                if unsafe { $crate::install_host(host) } {
                    &raw const $descriptor
                } else {
                    ::std::ptr::null()
                }
            })
            .unwrap_or(::std::ptr::null())
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of};

    #[test]
    fn structures_share_the_header_prefix() {
        assert_eq!(size_of::<Header>(), 8);
        assert_eq!(offset_of!(HostApiV1, abi_version), 0);
        assert_eq!(offset_of!(HostApiV1, struct_size), 4);
        assert_eq!(offset_of!(CreateContextV1, abi_version), 0);
        assert_eq!(offset_of!(CreateContextV1, struct_size), 4);
        assert_eq!(offset_of!(WidgetDescriptorV1, abi_version), 0);
        assert_eq!(offset_of!(WidgetDescriptorV1, struct_size), 4);
        assert_eq!(offset_of!(LxbContextV1, struct_size), 4);
        assert_eq!(offset_of!(LxbFrameV1, struct_size), 4);
        // The LineXinBar fields were appended after the original layout.
        assert!(
            offset_of!(WidgetDescriptorV1, lxb_mark)
                > offset_of!(WidgetDescriptorV1, close_request)
        );
        assert!(align_of::<HostApiV1>() >= align_of::<Header>());
        assert!(align_of::<CreateContextV1>() >= align_of::<Header>());
        assert!(align_of::<WidgetDescriptorV1>() >= align_of::<Header>());
    }

    #[test]
    fn header_rejects_wrong_version_or_size() {
        let good = Header {
            abi_version: 1,
            struct_size: 64,
        };
        let wrong = Header {
            abi_version: 2,
            struct_size: 64,
        };
        unsafe {
            assert!(Header::is_compatible((&raw const good).cast(), 64));
            assert!(!Header::is_compatible((&raw const good).cast(), 65));
            assert!(!Header::is_compatible((&raw const wrong).cast(), 8));
            assert!(!Header::is_compatible(std::ptr::null(), 8));
        }
    }

    #[test]
    fn tr_without_host_is_identity() {
        assert_eq!(tr("system_information", "Kernel"), "Kernel");
    }
}
