//! One frame of a window: the safe face of `lxb_page`.

use crate::sys::{Lxb, RawPage, load};
use crate::types::{
    Action, Align, Fit, Hold, Metric, Overlay, PickerSelection, Press, Role, Sound, Surface, Text,
};
use std::ffi::{CStr, CString, c_char, c_float, c_int, c_uint, c_ulong, c_void};
use std::marker::PhantomData;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

/// Call `$name` if the installed library has it, otherwise answer `$default`.
macro_rules! call {
    ($lxb:expr, $name:ident($($argument:expr),* $(,)?) else $default:expr) => {
        match $lxb.sym.$name {
            // SAFETY: the arguments are valid for the duration of the call
            // (the page belongs to the frame being drawn, strings and
            // rectangles are owned by the caller's stack).
            Some(function) => unsafe { function($($argument),*) },
            None => $default,
        }
    };
}
pub(crate) use call;

/// A string for C. An interior NUL, which C cannot carry, is dropped.
pub(crate) fn c_string(text: &str) -> CString {
    CString::new(text).unwrap_or_else(|err| {
        let mut bytes = err.into_vec();
        bytes.retain(|byte| *byte != 0);
        CString::new(bytes).unwrap_or_default()
    })
}

fn c_path(path: &Path) -> CString {
    let mut bytes = path.as_os_str().as_bytes().to_vec();
    bytes.retain(|byte| *byte != 0);
    CString::new(bytes).unwrap_or_default()
}

/// A file's path for C, or `None` when C would read another file: a NUL in
/// it would end the path early (no file's name holds one).
fn c_file(path: &Path) -> Option<CString> {
    CString::new(path.as_os_str().as_bytes()).ok()
}

/// A list of strings for C, kept alive as long as the value.
struct CList {
    _owned: Vec<CString>,
    pointers: Vec<*const c_char>,
}

impl CList {
    fn new(items: &[&str]) -> Self {
        let owned: Vec<CString> = items.iter().map(|item| c_string(item)).collect();
        let pointers = owned.iter().map(|item| item.as_ptr()).collect();
        Self {
            _owned: owned,
            pointers,
        }
    }

    fn as_ptr(&self) -> *const *const c_char {
        self.pointers.as_ptr()
    }

    fn len(&self) -> c_ulong {
        self.pointers.len() as c_ulong
    }
}

/// The page of one frame. A handle: copying it copies the pointer, and it is
/// valid only for the frame it was handed to (the lifetime says so).
///
/// Every method is a wrapper of the C function of the same name (`width` is
/// `lxb_page_width`, `draw_row` is `lxb_draw_row`); one the installed library
/// lacks does nothing and answers a neutral value. Rectangles are
/// `[x, y, width, height]` in window pixels.
#[derive(Clone, Copy)]
pub struct Page<'frame> {
    raw: NonNull<RawPage>,
    lxb: &'static Lxb,
    _frame: PhantomData<&'frame mut RawPage>,
}

impl<'frame> Page<'frame> {
    pub(crate) fn new(raw: NonNull<RawPage>, lxb: &'static Lxb) -> Self {
        Self {
            raw,
            lxb,
            _frame: PhantomData,
        }
    }

    /// Borrow a page handed over by somebody else, such as
    /// [`LxbFrameV1::page`](linexin_plugin_api::LxbFrameV1). Answers `None`
    /// for null, or when this process cannot open `liblxb_app.so`.
    ///
    /// # Safety
    /// `pointer` must be an `lxb_page*` of this process's `liblxb_app.so`,
    /// valid for the whole of `'frame` (the frame being drawn).
    pub unsafe fn from_raw(pointer: *mut c_void) -> Option<Page<'frame>> {
        let raw = NonNull::new(pointer.cast::<RawPage>())?;
        Some(Self::new(raw, load().ok()?))
    }

    /// The `lxb_page*`, to hand to code that draws with the C API itself.
    pub fn as_raw(&self) -> *mut c_void {
        self.raw.as_ptr().cast()
    }

    /// The libraries this page belongs to.
    pub fn lxb(&self) -> &'static Lxb {
        self.lxb
    }

    fn mut_ptr(&self) -> *mut RawPage {
        self.raw.as_ptr()
    }

    fn ptr(&self) -> *const RawPage {
        self.raw.as_ptr()
    }

    // -- what a frame is ------------------------------------------------------

    pub fn width(&self) -> f32 {
        call!(self.lxb, lxb_page_width(self.ptr()) else 0.0)
    }

    pub fn height(&self) -> f32 {
        call!(self.lxb, lxb_page_height(self.ptr()) else 0.0)
    }

    /// Seconds since the window opened.
    pub fn seconds(&self) -> f32 {
        call!(self.lxb, lxb_page_seconds(self.ptr()) else 0.0)
    }

    // -- input -------------------------------------------------------------------

    /// The next action since the last frame (driven applications only).
    /// Indices this crate does not know are skipped.
    pub fn action(&self) -> Option<Action> {
        loop {
            let raw = call!(self.lxb, lxb_page_action(self.mut_ptr()) else -1);
            let raw = u32::try_from(raw).ok()?;
            if let Some(action) = Action::from_raw(raw) {
                return Some(action);
            }
        }
    }

    /// Every action since the last frame, in the order they happened.
    pub fn actions(&self) -> Vec<Action> {
        std::iter::from_fn(|| self.action()).collect()
    }

    /// Hand the window `action` as a key or a controller would
    /// (`lxb_page_act`): a question or a menu over the page takes it, and
    /// otherwise the page reads it with [`Page::action`]. Answers whether the
    /// library could (0.9.3 lacks it).
    pub fn act(&self, action: Action) -> bool {
        match self.lxb.sym.lxb_page_act {
            Some(function) => {
                // SAFETY: the page belongs to the frame being drawn.
                unsafe { function(self.mut_ptr(), action.ulong()) };
                true
            }
            None => false,
        }
    }

    /// Whether the toolkit counts the session as low-end hardware: what moves
    /// should be put where it is going at once (`false` with an older
    /// library).
    pub fn low_end(&self) -> bool {
        call!(self.lxb, lxb_page_low_end(self.ptr()) else 0) != 0
    }

    /// How far a wheel moved over the spot `id`, positive downwards.
    pub fn scrolled(&self, id: u32) -> i32 {
        call!(self.lxb, lxb_page_scrolled(self.ptr(), id as c_uint) else 0)
    }

    /// Where the pointer rests over the page (`None` with an older library).
    pub fn pointer(&self) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        let found = call!(self.lxb, lxb_page_pointer(self.ptr(), out.as_mut_ptr()) else 0);
        (found != 0).then_some(out)
    }

    /// Whether the pointer moved over the page since the last frame.
    pub fn pointer_moved(&self) -> bool {
        call!(self.lxb, lxb_page_pointer_moved(self.ptr()) else 0) != 0
    }

    /// The control or spot the pointer moved onto since the last frame.
    pub fn hovered(&self) -> Option<u32> {
        let id = call!(self.lxb, lxb_page_hovered(self.ptr()) else -1);
        u32::try_from(id).ok()
    }

    /// Put the flow's light on its control `index` (driven applications).
    pub fn focus(&self, index: usize) {
        call!(self.lxb, lxb_page_focus(self.mut_ptr(), index as c_ulong) else ())
    }

    pub fn focused(&self) -> usize {
        call!(self.lxb, lxb_page_focused(self.ptr()) else 0) as usize
    }

    /// Draw the page's own light travelling to `rect`; answers where it is
    /// this frame.
    pub fn glide(&self, rect: [f32; 4], strength: f32) -> [f32; 4] {
        let mut out = rect;
        call!(self.lxb, lxb_page_glide(self.mut_ptr(), rect.as_ptr(), strength, out.as_mut_ptr()) else ());
        out
    }

    /// The same light, appearing at `rect` instead of travelling there.
    pub fn place(&self, rect: [f32; 4], strength: f32) -> [f32; 4] {
        let mut out = rect;
        call!(self.lxb, lxb_page_place(self.mut_ptr(), rect.as_ptr(), strength, out.as_mut_ptr()) else ());
        out
    }

    /// How to draw a control of your own: resting, lit or pressing.
    /// The light carried to `rect` (put there at once with `appears`) and
    /// drawn in its own shape, corners rounded by `radius`
    /// (`lxb_page_light_shaped`, lxb-toolkit 0.9.4): a card or a
    /// picture lit as itself. Where the toolkit cannot, the rectangle is lit
    /// with accent glass where it stands. Where the light is now.
    pub fn light_shaped(
        &self,
        rect: [f32; 4],
        radius: f32,
        strength: f32,
        appears: bool,
    ) -> [f32; 4] {
        if let Some(function) = self.lxb.sym.lxb_page_light_shaped {
            let mut out = rect;
            // SAFETY: the page belongs to the frame being drawn; both arrays
            // are four floats long.
            unsafe {
                function(
                    self.mut_ptr(),
                    rect.as_ptr(),
                    radius,
                    strength,
                    c_int::from(appears),
                    out.as_mut_ptr(),
                )
            };
            self.light_at(out);
            return out;
        }
        self.draw_card(rect, Surface::Control, Role::Accent, 0.85 * strength);
        self.light_at(rect);
        rect
    }

    pub fn press(&self, lit: bool) -> Press {
        let raw =
            call!(self.lxb, lxb_page_press(self.ptr(), c_int::from(lit)) else c_ulong::from(lit));
        Press::from_raw(raw)
    }

    /// Whether a press landed on the spot `id` (answered once).
    pub fn pressed(&self, id: u32) -> bool {
        call!(self.lxb, lxb_page_pressed(self.mut_ptr(), id as c_uint) else 0) != 0
    }

    /// Where the pointer is while a press that began on `id` is held.
    pub fn dragging(&self, id: u32) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        let found =
            call!(self.lxb, lxb_page_dragging(self.ptr(), id as c_uint, out.as_mut_ptr()) else 0);
        (found != 0).then_some(out)
    }

    /// What is under a point of the frame on screen.
    pub fn at(&self, x: f32, y: f32) -> Option<u32> {
        u32::try_from(call!(self.lxb, lxb_page_at(self.ptr(), x, y) else -1)).ok()
    }

    // -- the window --------------------------------------------------------------

    /// Close the window at the end of this frame.
    /// Whether the toolkit lets a page take text (`lxb_page_taking_text`,
    /// lxb-toolkit 0.9.4); where it does not, a field can be
    /// shown but not typed into.
    pub fn takes_typing(&self) -> bool {
        self.lxb.sym.lxb_page_taking_text.is_some() && self.lxb.sym.lxb_page_typed.is_some()
    }

    /// A field takes text this frame, standing at `rect`: keys type into it
    /// rather than walking the page, and LineXinBar raises its on-screen
    /// keyboard over it with a controller in hand. Say it on every frame the
    /// field takes text.
    pub fn taking_text(&self, rect: Option<[f32; 4]>) {
        let rect = rect.as_ref().map_or(std::ptr::null(), |rect| rect.as_ptr());
        call!(self.lxb, lxb_page_taking_text(self.mut_ptr(), 1, rect) else ())
    }

    /// What was typed since the last frame, in order: letters, and U+0008
    /// for each Backspace (see [`crate::typed::apply`]).
    pub fn typed(&self) -> String {
        let mut text = crate::typed::scripted(self.seconds());
        let raw = call!(self.lxb, lxb_page_typed(self.mut_ptr()) else std::ptr::null_mut());
        if !raw.is_null() {
            // SAFETY: a non-null answer is a NUL-terminated string we own.
            text.push_str(&unsafe { CStr::from_ptr(raw) }.to_string_lossy());
            call!(self.lxb, lxb_app_string_free(raw) else ());
        }
        text
    }

    /// Where LineXinBar's on-screen keyboard stands over the window.
    pub fn board(&self) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        (call!(self.lxb, lxb_page_board(self.ptr(), out.as_mut_ptr()) else 0) != 0).then_some(out)
    }

    pub fn quit(&self) {
        call!(self.lxb, lxb_page_quit(self.mut_ptr()) else ())
    }

    pub fn keep_awake(&self, hold: Hold) {
        call!(self.lxb, lxb_page_keep_awake(self.mut_ptr(), c_int::from(hold.screen), c_int::from(hold.sleep)) else ())
    }

    /// Draw the next frame no later than this (never sooner than 33 ms).
    pub fn redraw_within(&self, seconds: f32) {
        call!(self.lxb, lxb_page_redraw_within(self.mut_ptr(), seconds) else ())
    }

    pub fn play(&self, sound: Sound) {
        call!(self.lxb, lxb_page_play(self.mut_ptr(), sound.ulong()) else ())
    }

    pub fn volume(&self, value: f32, muted: bool) {
        call!(self.lxb, lxb_page_volume(self.mut_ptr(), value, c_int::from(muted)) else ())
    }

    // -- the flow ------------------------------------------------------------------

    /// Where the flow has got to.
    pub fn cursor(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        call!(self.lxb, lxb_page_cursor(self.ptr(), out.as_mut_ptr()) else ());
        out
    }

    /// Lay the flow out inside `rect`, from its top.
    pub fn set_cursor(&self, rect: [f32; 4]) {
        call!(self.lxb, lxb_page_set_cursor(self.mut_ptr(), rect.as_ptr()) else ())
    }

    pub fn title(&self, text: &str) {
        let text = c_string(text);
        call!(self.lxb, lxb_page_title(self.mut_ptr(), text.as_ptr()) else ())
    }

    pub fn heading(&self, text: &str) {
        let text = c_string(text);
        call!(self.lxb, lxb_page_heading(self.mut_ptr(), text.as_ptr()) else ())
    }

    pub fn text(&self, text: &str) {
        let text = c_string(text);
        call!(self.lxb, lxb_page_text(self.mut_ptr(), text.as_ptr()) else ())
    }

    pub fn note(&self, text: &str) {
        let text = c_string(text);
        call!(self.lxb, lxb_page_note(self.mut_ptr(), text.as_ptr()) else ())
    }

    pub fn icon(&self, name: &str) {
        let name = c_string(name);
        call!(self.lxb, lxb_page_icon(self.mut_ptr(), name.as_ptr()) else ())
    }

    pub fn head(&self, icon: &str, title: &str) {
        let (icon, title) = (c_string(icon), c_string(title));
        call!(self.lxb, lxb_page_head(self.mut_ptr(), icon.as_ptr(), title.as_ptr()) else ())
    }

    pub fn rule(&self) {
        call!(self.lxb, lxb_page_rule(self.mut_ptr()) else ())
    }

    pub fn gap(&self) {
        call!(self.lxb, lxb_page_gap(self.mut_ptr()) else ())
    }

    pub fn button(&self, label: &str) -> bool {
        let label = c_string(label);
        call!(self.lxb, lxb_page_button(self.mut_ptr(), label.as_ptr()) else 0) != 0
    }

    pub fn row(&self, label: &str) -> bool {
        let label = c_string(label);
        call!(self.lxb, lxb_page_row(self.mut_ptr(), label.as_ptr()) else 0) != 0
    }

    pub fn item(&self, label: &str) -> bool {
        let label = c_string(label);
        call!(self.lxb, lxb_page_item(self.mut_ptr(), label.as_ptr()) else 0) != 0
    }

    pub fn row_value(&self, label: &str, value: &str) -> bool {
        let (label, value) = (c_string(label), c_string(value));
        call!(self.lxb, lxb_page_row_value(self.mut_ptr(), label.as_ptr(), value.as_ptr()) else 0)
            != 0
    }

    /// Where the light stands, for a page that draws its own controls; a
    /// menu grows out of it.
    pub fn light_at(&self, rect: [f32; 4]) {
        call!(self.lxb, lxb_page_light_at(self.mut_ptr(), rect.as_ptr()) else ())
    }

    // -- panels ----------------------------------------------------------------------

    /// Raise a context menu over the control the light is on.
    pub fn menu(&self, title: Option<&str>, commands: &[&str]) {
        let title = title.map(c_string);
        let list = CList::new(commands);
        call!(self.lxb, lxb_page_menu(self.mut_ptr(), optional(&title), list.as_ptr(), list.len()) else ())
    }

    /// The same menu, with the command in force (`marked`) wearing the
    /// chosen mark and the light on it as it opens.
    pub fn menu_marked(&self, title: Option<&str>, commands: &[&str], marked: Option<usize>) {
        let title = title.map(c_string);
        let list = CList::new(commands);
        let marked = marked.map_or(c_ulong::MAX, |index| index as c_ulong);
        call!(self.lxb, lxb_page_menu_marked(self.mut_ptr(), optional(&title), list.as_ptr(), list.len(), marked) else ())
    }

    /// A menu of acts with the light on command `row` as it opens
    /// (`lxb_page_menu_on`; an older library opens it on the first).
    pub fn menu_on(&self, title: Option<&str>, commands: &[&str], row: usize) {
        let title = title.map(c_string);
        let list = CList::new(commands);
        if self.lxb.sym.lxb_page_menu_on.is_some() {
            call!(self.lxb, lxb_page_menu_on(self.mut_ptr(), optional(&title), list.as_ptr(), list.len(), row as c_ulong) else ())
        } else {
            call!(self.lxb, lxb_page_menu(self.mut_ptr(), optional(&title), list.as_ptr(), list.len()) else ())
        }
    }

    /// Which command was pressed, on the frame it was pressed.
    pub fn chose(&self) -> Option<usize> {
        usize::try_from(call!(self.lxb, lxb_page_chose(self.mut_ptr()) else -1)).ok()
    }

    /// Ask a question, modal until it is answered.
    pub fn ask(&self, title: &str, body: &str, answers: &[&str]) {
        let (title, body) = (c_string(title), c_string(body));
        let list = CList::new(answers);
        call!(self.lxb, lxb_page_ask(self.mut_ptr(), title.as_ptr(), body.as_ptr(), list.as_ptr(), list.len()) else ())
    }

    /// Which answer was given, on the frame it was given.
    pub fn answered(&self) -> Option<usize> {
        usize::try_from(call!(self.lxb, lxb_page_answered(self.mut_ptr()) else -1)).ok()
    }

    /// Ask for one file, or a folder. False when something else owns input.
    pub fn pick(&self, selection: PickerSelection, directory: impl AsRef<Path>) -> bool {
        let directory = c_path(directory.as_ref());
        call!(self.lxb, lxb_page_pick(self.mut_ptr(), selection.ulong(), directory.as_ptr()) else 0)
            != 0
    }

    pub fn pick_many(&self, selection: PickerSelection, directory: impl AsRef<Path>) -> bool {
        let directory = c_path(directory.as_ref());
        call!(self.lxb, lxb_page_pick_many(self.mut_ptr(), selection.ulong(), directory.as_ptr()) else 0)
            != 0
    }

    pub fn save(&self, name: &str, directory: impl AsRef<Path>) -> bool {
        let (name, directory) = (c_string(name), c_path(directory.as_ref()));
        call!(self.lxb, lxb_page_save(self.mut_ptr(), name.as_ptr(), directory.as_ptr()) else 0)
            != 0
    }

    /// The path a question was answered with, once.
    pub fn picked(&self) -> Option<PathBuf> {
        let raw = call!(self.lxb, lxb_page_picked(self.mut_ptr()) else std::ptr::null_mut());
        self.owned_path(raw)
    }

    /// The next path of a many-files answer, until there are none left.
    pub fn picked_next(&self) -> Option<PathBuf> {
        let raw = call!(self.lxb, lxb_page_picked_next(self.mut_ptr()) else std::ptr::null_mut());
        self.owned_path(raw)
    }

    fn owned_path(&self, raw: *mut c_char) -> Option<PathBuf> {
        if raw.is_null() {
            return None;
        }
        // SAFETY: a non-null answer is a NUL-terminated string we own.
        let bytes = unsafe { CStr::from_ptr(raw) }.to_bytes().to_vec();
        call!(self.lxb, lxb_app_string_free(raw) else ());
        Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
    }

    // -- drawing -----------------------------------------------------------------------

    pub fn draw_pane(&self, rect: [f32; 4], overlay: Overlay) {
        call!(self.lxb, lxb_draw_pane(self.mut_ptr(), rect.as_ptr(), overlay.ulong()) else ())
    }

    pub fn draw_card(&self, rect: [f32; 4], surface: Surface, role: Role, alpha: f32) {
        call!(self.lxb, lxb_draw_card(self.mut_ptr(), rect.as_ptr(), surface.ulong(), role.ulong(), alpha) else ())
    }

    pub fn draw_label(&self, rect: [f32; 4], text: Text, string: &str, role: Role, align: Align) {
        let string = c_string(string);
        call!(self.lxb, lxb_draw_label(self.mut_ptr(), rect.as_ptr(), text.ulong(), string.as_ptr(), role.ulong(), align.ulong()) else ())
    }

    /// A sentence wrapped to the rectangle's width; answers its height.
    pub fn draw_paragraph(&self, rect: [f32; 4], string: &str, role: Role) -> f32 {
        let string = c_string(string);
        call!(self.lxb, lxb_draw_paragraph(self.mut_ptr(), rect.as_ptr(), string.as_ptr(), role.ulong()) else 0.0)
    }

    pub fn draw_icon(&self, rect: [f32; 4], name: &str) {
        let name = c_string(name);
        call!(self.lxb, lxb_draw_icon(self.mut_ptr(), rect.as_ptr(), name.as_ptr()) else ())
    }

    pub fn draw_rule(&self, rect: [f32; 4], role: Role) {
        call!(self.lxb, lxb_draw_rule(self.mut_ptr(), rect.as_ptr(), role.ulong()) else ())
    }

    pub fn draw_button(&self, rect: [f32; 4], label: &str, press: Press) {
        let label = c_string(label);
        call!(self.lxb, lxb_draw_button(self.mut_ptr(), rect.as_ptr(), label.as_ptr(), press.raw()) else ())
    }

    pub fn draw_row(&self, rect: [f32; 4], name: &str, value: Option<&str>, press: Press) {
        let name = c_string(name);
        let value = value.map(c_string);
        call!(self.lxb, lxb_draw_row(self.mut_ptr(), rect.as_ptr(), name.as_ptr(), optional(&value), press.raw()) else ())
    }

    /// The lit capsule, without a control under it and without travelling.
    pub fn draw_selection(&self, rect: [f32; 4], strength: f32) {
        call!(self.lxb, lxb_draw_selection(self.mut_ptr(), rect.as_ptr(), strength) else ())
    }

    /// Write down where something you drew went, for the pointer.
    pub fn draw_spot(&self, id: u32, rect: [f32; 4]) {
        call!(self.lxb, lxb_draw_spot(self.mut_ptr(), id as c_uint, rect.as_ptr()) else ())
    }

    /// Blur and fade the ends of a scrolling area, after drawing it.
    pub fn draw_soft_edges(&self, rect: [f32; 4], band: f32, top: f32, bottom: f32) {
        call!(self.lxb, lxb_draw_soft_edges(self.mut_ptr(), rect.as_ptr(), band, top, bottom) else ())
    }

    /// The same at the left and right edges of an area.
    pub fn draw_soft_side_edges(&self, rect: [f32; 4], band: f32, left: f32, right: f32) {
        call!(self.lxb, lxb_draw_soft_side_edges(self.mut_ptr(), rect.as_ptr(), band, left, right) else ())
    }

    /// The chosen mark (a tick cut through a bead) in the square `rect`.
    pub fn draw_chosen(&self, rect: [f32; 4], alpha: f32) {
        call!(self.lxb, lxb_draw_chosen(self.mut_ptr(), rect.as_ptr(), alpha) else ())
    }

    /// A switch, on or off; `ink` is 1 on a control that can be used.
    pub fn draw_switch(&self, rect: [f32; 4], on: bool, ink: f32) {
        call!(self.lxb, lxb_draw_switch(self.mut_ptr(), rect.as_ptr(), c_int::from(on), ink) else ())
    }

    /// Whether the toolkit draws switches (`lxb_draw_switch`, lxb-toolkit
    /// 0.9.4); where it does not, say what a switch would show in words.
    pub fn draws_switches(&self) -> bool {
        self.lxb.sym.lxb_draw_switch.is_some()
    }

    /// How long a switch `height` tall is drawn.
    pub fn switch_width(&self, height: f32) -> f32 {
        call!(self.lxb, lxb_switch_width(height) else height * 1.75)
    }

    /// Whether the toolkit draws pictures from files (`lxb_draw_picture`,
    /// lxb-toolkit 0.9.4).
    pub fn draws_pictures(&self) -> bool {
        self.lxb.sym.lxb_draw_picture.is_some()
    }

    /// A picture from a file on this disk (PNG, JPEG, WebP, SVG) in `rect`,
    /// fitted as `fit` says, its corners rounded by `radius`, at `alpha`.
    /// `true` once it is drawn; `false` while the toolkit is still reading it,
    /// when it cannot be read, or where the toolkit draws no pictures (draw
    /// what stands in for it then: see [`Emblem`](crate::Emblem)).
    pub fn draw_picture(
        &self,
        rect: [f32; 4],
        radius: f32,
        path: &Path,
        fit: Fit,
        alpha: f32,
    ) -> bool {
        let Some(path) = c_file(path) else {
            return false;
        };
        call!(self.lxb, lxb_draw_picture(self.mut_ptr(), rect.as_ptr(), radius, path.as_ptr(), fit.ulong(), alpha) else 0)
            != 0
    }

    // -- sizes -----------------------------------------------------------------------------

    pub fn metric(&self, metric: Metric) -> f32 {
        call!(self.lxb, lxb_page_metric(self.ptr(), metric.ulong()) else 0.0)
    }

    /// A length written against the 1080-pixel reference, for this window.
    pub fn scaled(&self, reference: f32) -> f32 {
        call!(self.lxb, lxb_page_scaled(self.ptr(), reference as c_float) else reference)
    }

    /// How tall one line of a size is, air included.
    pub fn line(&self, text: Text) -> f32 {
        call!(self.lxb, lxb_page_line(self.ptr(), text.ulong()) else 0.0)
    }

    /// How wide a string is at a size.
    pub fn measure(&self, text: Text, string: &str) -> f32 {
        let string = c_string(string);
        call!(self.lxb, lxb_page_measure(self.mut_ptr(), text.ulong(), string.as_ptr()) else 0.0)
    }

    /// `string` cut to `room` with an ellipsis where it does not fit.
    pub fn fit(&self, text: Text, string: &str, room: f32) -> String {
        crate::wrap::fit(string, room, |candidate| self.measure(text, candidate))
    }
}

fn optional(string: &Option<CString>) -> *const c_char {
    string
        .as_ref()
        .map_or(std::ptr::null(), |string| string.as_ptr())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interior_nul_is_dropped_not_fatal() {
        assert_eq!(c_string("Kernel").as_bytes(), b"Kernel");
        assert_eq!(c_string("Ker\0nel").as_bytes(), b"Kernel");
        assert_eq!(c_string("\0").as_bytes(), b"");
        assert_eq!(c_path(Path::new("/tmp/a\0b")).as_bytes(), b"/tmp/ab");
    }

    #[test]
    fn a_picture_whose_path_holds_a_nul_is_not_another_file() {
        assert!(c_file(Path::new("/tmp/a\0b.png")).is_none());
        assert_eq!(
            c_file(Path::new("/tmp/a b.png")).map(CString::into_bytes),
            Some(b"/tmp/a b.png".to_vec())
        );
    }

    #[test]
    fn a_list_keeps_its_strings_alive() {
        let list = CList::new(&["Leave", "Stay"]);
        assert_eq!(list.len(), 2);
        // SAFETY: the pointers belong to `list`, alive here.
        let second = unsafe { CStr::from_ptr(*list.as_ptr().add(1)) };
        assert_eq!(second.to_bytes(), b"Stay");
    }
}
