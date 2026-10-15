//! The two libraries, opened once per process with `dlopen` and never closed,
//! and every function this crate calls, resolved one by one.

use libloading::Library;
use std::ffi::{CStr, c_char, c_float, c_int, c_uint, c_ulong, c_void};
use std::marker::{PhantomData, PhantomPinned};
use std::sync::OnceLock;

/// File name of the library carrying the window, the controls and the page.
pub const APP_LIBRARY: &str = "liblxb_app.so";
/// File name of the library carrying the language's answers (names, layout).
pub const TOOLKIT_LIBRARY: &str = "liblxb_toolkit.so";

/// `lxb_app` (opaque).
#[repr(C)]
pub struct RawApp {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `lxb_page` (opaque).
#[repr(C)]
pub struct RawPage {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `lxb_page_fn`.
pub type PageFn = extern "C" fn(page: *mut RawPage, data: *mut c_void);

/// `lxb_beside`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RawBeside {
    pub page_x: c_float,
    pub page_w: c_float,
    pub reach: c_float,
}

/// `lxb_slide`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RawSlide {
    pub at: c_float,
    pub speed: c_float,
    pub target: c_float,
    pub placed: u8,
}

/// `lxb_menu` (only `row` is read here, the rest keeps the layout).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RawMenu {
    pub width: c_float,
    pub extra_width: c_float,
    pub row: c_float,
    pub stacked_row: c_float,
    pub title: c_float,
    pub title_size: c_float,
    pub label_size: c_float,
    pub detail_size: c_float,
    pub stamp_size: c_float,
    pub group_gap: c_float,
    pub gap: c_float,
    pub glow_reach: c_float,
    pub scroll_strip: c_float,
    pub scroll_arrow: c_float,
    pub dim: c_float,
    pub scrim: c_float,
    pub depth: c_float,
    pub content_in: c_float,
    pub panel_in: c_float,
    pub icon: c_float,
    pub glyph: c_float,
    pub aside: c_float,
    pub aside_gap: c_float,
    pub aside_glyph: c_float,
    pub max_lines: c_uint,
}

type Rect = *const c_float;
type Out = *mut c_float;
type Str = *const c_char;
type App = *mut RawApp;
type Page = *mut RawPage;
type ConstPage = *const RawPage;

macro_rules! symbols {
    (@library app, $app:expr, $toolkit:expr) => { Some($app) };
    (@library toolkit, $app:expr, $toolkit:expr) => { $toolkit };
    ($( $library:ident fn $name:ident ( $($argument:ty),* ) $(-> $result:ty)? ; )*) => {
        /// Every function this crate may call; `None` where the installed
        /// library is older than the function.
        #[allow(non_snake_case)]
        pub struct Symbols {
            $( pub $name: Option<unsafe extern "C" fn($($argument),*) $(-> $result)?>, )*
        }

        impl Symbols {
            fn resolve(app: &Library, toolkit: Option<&Library>) -> Self {
                Self {
                    $( $name: lookup(
                        symbols!(@library $library, app, toolkit),
                        concat!(stringify!($name), "\0"),
                    ), )*
                }
            }

            /// Whether `name` (for example `lxb_page_hovered`) was found.
            pub fn has(&self, name: &str) -> bool {
                match name {
                    $( stringify!($name) => self.$name.is_some(), )*
                    _ => false,
                }
            }

            /// The names of every function this crate knows about.
            pub const NAMES: &'static [&'static str] = &[$(stringify!($name)),*];
        }
    };
}

fn lookup<T: Copy>(library: Option<&Library>, name: &str) -> Option<T> {
    // SAFETY: the type is the one the header declares for `name`, and the
    // library is never unloaded, so the copied pointer stays valid.
    unsafe {
        library?
            .get::<T>(name.as_bytes())
            .ok()
            .map(|symbol| *symbol)
    }
}

symbols! {
    app fn lxb_app_new(Str, Str) -> App;
    app fn lxb_app_size(App, f64, f64);
    app fn lxb_app_plain(App);
    app fn lxb_app_driven(App);
    app fn lxb_app_run(App, Option<PageFn>, *mut c_void) -> c_int;
    app fn lxb_app_shot(App, Str, c_uint, c_uint, c_float, Option<PageFn>, *mut c_void) -> c_int;
    app fn lxb_app_shot_after(App, Str, c_uint, c_uint, c_float, c_uint, Option<PageFn>, *mut c_void) -> c_int;
    app fn lxb_app_low_end(App);
    app fn lxb_app_mark(App, Str, *const c_void, c_ulong) -> c_int;
    app fn lxb_app_trouble(*const RawApp) -> Str;
    app fn lxb_app_free(App);
    app fn lxb_app_string_free(*mut c_char);

    app fn lxb_page_width(ConstPage) -> c_float;
    app fn lxb_page_height(ConstPage) -> c_float;
    app fn lxb_page_seconds(ConstPage) -> c_float;
    app fn lxb_page_action(Page) -> c_int;
    app fn lxb_page_act(Page, c_ulong);
    app fn lxb_page_low_end(ConstPage) -> c_int;
    app fn lxb_page_scrolled(ConstPage, c_uint) -> c_int;
    app fn lxb_page_pointer(ConstPage, Out) -> c_int;
    app fn lxb_page_pointer_moved(ConstPage) -> c_int;
    app fn lxb_page_hovered(ConstPage) -> c_int;
    app fn lxb_page_focus(Page, c_ulong);
    app fn lxb_page_focused(ConstPage) -> c_ulong;
    app fn lxb_page_glide(Page, Rect, c_float, Out);
    app fn lxb_page_place(Page, Rect, c_float, Out);
    app fn lxb_page_press(ConstPage, c_int) -> c_ulong;
    app fn lxb_page_light_shaped(Page, Rect, c_float, c_float, c_int, Out);
    app fn lxb_page_pressed(Page, c_uint) -> c_int;
    app fn lxb_page_dragging(ConstPage, c_uint, Out) -> c_int;
    app fn lxb_page_quit(Page);
    app fn lxb_page_taking_text(Page, c_int, Rect);
    app fn lxb_page_typed(Page) -> *mut c_char;
    app fn lxb_page_board(ConstPage, Out) -> c_int;
    app fn lxb_page_keep_awake(Page, c_int, c_int);
    app fn lxb_page_redraw_within(Page, c_float);
    app fn lxb_page_play(Page, c_ulong);
    app fn lxb_page_volume(Page, c_float, c_int);
    app fn lxb_page_cursor(ConstPage, Out);
    app fn lxb_page_set_cursor(Page, Rect);
    app fn lxb_page_title(Page, Str);
    app fn lxb_page_heading(Page, Str);
    app fn lxb_page_text(Page, Str);
    app fn lxb_page_note(Page, Str);
    app fn lxb_page_icon(Page, Str);
    app fn lxb_page_head(Page, Str, Str);
    app fn lxb_page_rule(Page);
    app fn lxb_page_gap(Page);
    app fn lxb_page_button(Page, Str) -> c_int;
    app fn lxb_page_row(Page, Str) -> c_int;
    app fn lxb_page_item(Page, Str) -> c_int;
    app fn lxb_page_row_value(Page, Str, Str) -> c_int;
    app fn lxb_page_light_at(Page, Rect);
    app fn lxb_page_menu(Page, Str, *const Str, c_ulong);
    app fn lxb_page_menu_marked(Page, Str, *const Str, c_ulong, c_ulong);
    app fn lxb_page_menu_on(Page, Str, *const Str, c_ulong, c_ulong);
    app fn lxb_page_chose(Page) -> c_int;
    app fn lxb_page_ask(Page, Str, Str, *const Str, c_ulong);
    app fn lxb_page_answered(Page) -> c_int;
    app fn lxb_page_pick(Page, c_ulong, Str) -> c_int;
    app fn lxb_page_pick_many(Page, c_ulong, Str) -> c_int;
    app fn lxb_page_save(Page, Str, Str) -> c_int;
    app fn lxb_page_picked(Page) -> *mut c_char;
    app fn lxb_page_picked_next(Page) -> *mut c_char;
    app fn lxb_page_at(ConstPage, c_float, c_float) -> c_int;
    app fn lxb_page_metric(ConstPage, c_ulong) -> c_float;
    app fn lxb_page_scaled(ConstPage, c_float) -> c_float;
    app fn lxb_page_line(ConstPage, c_ulong) -> c_float;
    app fn lxb_page_measure(Page, c_ulong, Str) -> c_float;

    app fn lxb_draw_pane(Page, Rect, c_ulong);
    app fn lxb_draw_card(Page, Rect, c_ulong, c_ulong, c_float);
    app fn lxb_draw_label(Page, Rect, c_ulong, Str, c_ulong, c_ulong);
    app fn lxb_draw_paragraph(Page, Rect, Str, c_ulong) -> c_float;
    app fn lxb_draw_icon(Page, Rect, Str);
    app fn lxb_draw_rule(Page, Rect, c_ulong);
    app fn lxb_draw_button(Page, Rect, Str, c_ulong);
    app fn lxb_draw_row(Page, Rect, Str, Str, c_ulong);
    app fn lxb_draw_selection(Page, Rect, c_float);
    app fn lxb_draw_spot(Page, c_uint, Rect);
    app fn lxb_draw_soft_edges(Page, Rect, c_float, c_float, c_float);
    app fn lxb_draw_soft_side_edges(Page, Rect, c_float, c_float, c_float);
    app fn lxb_draw_chosen(Page, Rect, c_float);
    app fn lxb_draw_switch(Page, Rect, c_int, c_float);
    app fn lxb_switch_width(c_float) -> c_float;
    app fn lxb_draw_picture(Page, Rect, c_float, Str, c_ulong, c_float) -> c_int;

    toolkit fn lxb_version() -> Str;
    toolkit fn lxb_action_count() -> c_ulong;
    toolkit fn lxb_action_name(c_ulong) -> Str;
    toolkit fn lxb_role_count() -> c_ulong;
    toolkit fn lxb_role_name(c_ulong) -> Str;
    toolkit fn lxb_metric_count() -> c_ulong;
    toolkit fn lxb_metric_name(c_ulong) -> Str;
    toolkit fn lxb_text_count() -> c_ulong;
    toolkit fn lxb_text_name(c_ulong) -> Str;
    toolkit fn lxb_surface_count() -> c_ulong;
    toolkit fn lxb_surface_name(c_ulong) -> Str;
    toolkit fn lxb_overlay_count() -> c_ulong;
    toolkit fn lxb_overlay_name(c_ulong) -> Str;
    toolkit fn lxb_sound_count() -> c_ulong;
    toolkit fn lxb_sound_name(c_ulong) -> Str;
    toolkit fn lxb_glyph_count() -> c_ulong;
    toolkit fn lxb_glyph_name(c_ulong) -> Str;
    toolkit fn lxb_duration_named(Str) -> c_float;
    toolkit fn lxb_layout_beside(c_float, c_float, c_float, c_float, c_float) -> RawBeside;
    toolkit fn lxb_peek() -> c_float;
    toolkit fn lxb_slide_follow(*mut RawSlide, c_float, c_float) -> c_float;
    toolkit fn lxb_slide_moving(*const RawSlide) -> c_int;
    toolkit fn lxb_glide(*mut c_float, *mut c_float, c_float, c_float);
    toolkit fn lxb_context_menu() -> RawMenu;
    toolkit fn lxb_menu_row_padding() -> c_float;
}

/// What a library that is not `liblxb_app` at all would lack.
const CORE: &[&str] = &[
    "lxb_app_new",
    "lxb_app_free",
    "lxb_app_run",
    "lxb_page_width",
    "lxb_page_height",
];

/// The opened libraries. Lives for the whole process: the handles are kept
/// in a `static` and never closed, because a page, a widget or a thread of
/// the toolkit's own may still run code from them.
pub struct Lxb {
    _app: Library,
    toolkit: Option<Library>,
    /// The resolved functions.
    pub sym: Symbols,
}

static LOADED: OnceLock<Result<Lxb, String>> = OnceLock::new();

/// Open `liblxb_app.so` (and `liblxb_toolkit.so`) the first time it is asked,
/// and answer the same libraries every time after.
///
/// Fails with a sentence saying what is missing on a machine without
/// LineXinBar. A function missing from an older installed library does not
/// fail it: its wrapper does nothing and answers a neutral value, and
/// [`Lxb::has`] says so beforehand.
pub fn load() -> Result<&'static Lxb, String> {
    LOADED.get_or_init(Lxb::open).as_ref().map_err(Clone::clone)
}

impl Lxb {
    fn open() -> Result<Self, String> {
        // SAFETY: loading runs the libraries' initialisers, which is what
        // they are for; they are opened by their plain names so that every
        // module of this process reaches the same instance.
        let app = unsafe { Library::new(APP_LIBRARY) }
            .map_err(|err| format!("LineXinBar's {APP_LIBRARY} is not available: {err}"))?;
        let toolkit = unsafe { Library::new(TOOLKIT_LIBRARY) }.ok();
        let sym = Symbols::resolve(&app, toolkit.as_ref());
        let missing: Vec<&str> = CORE.iter().copied().filter(|name| !sym.has(name)).collect();
        if !missing.is_empty() {
            return Err(format!("{APP_LIBRARY} lacks {}", missing.join(", ")));
        }
        Ok(Self {
            _app: app,
            toolkit,
            sym,
        })
    }

    /// Whether the installed libraries export `name` (any function of
    /// `lxb_app.h`, or the `lxb_toolkit.h` ones this crate uses).
    pub fn has(&self, name: &str) -> bool {
        self.sym.has(name)
    }

    /// The known functions the installed libraries do not export.
    pub fn missing(&self) -> Vec<&'static str> {
        Symbols::NAMES
            .iter()
            .copied()
            .filter(|name| !self.sym.has(name))
            .collect()
    }

    /// Whether `liblxb_toolkit.so` opened as well.
    pub fn has_toolkit(&self) -> bool {
        self.toolkit.is_some()
    }
}

/// A static string answered by the toolkit, if it is valid UTF-8.
pub(crate) fn static_str(pointer: *const c_char) -> Option<&'static str> {
    if pointer.is_null() {
        return None;
    }
    // SAFETY: the toolkit's names are static and the library is never closed.
    unsafe { CStr::from_ptr(pointer) }.to_str().ok()
}
