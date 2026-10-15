//! The window: the safe face of `lxb_app`.

use crate::page::{Page, c_string, call};
use crate::sys::{Lxb, RawApp, RawPage, load};
use std::any::Any;
use std::ffi::{CStr, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::Path;
use std::ptr::NonNull;

/// An lxb-toolkit application: a Wayland window carrying the shell's
/// wallpaper and glass, driven by keyboard, pointer and every controller.
pub struct App {
    raw: NonNull<RawApp>,
    lxb: &'static Lxb,
}

impl App {
    /// A new application (`lxb_app_new`). Fails only when the library
    /// cannot be opened.
    pub fn new(app_id: &str, title: &str) -> Result<Self, String> {
        let lxb = load()?;
        let (app_id, title) = (c_string(app_id), c_string(title));
        let raw =
            call!(lxb, lxb_app_new(app_id.as_ptr(), title.as_ptr()) else std::ptr::null_mut());
        let raw = NonNull::new(raw).ok_or("lxb_app_new answered null")?;
        Ok(Self { raw, lxb })
    }

    /// The window size asked for, in logical pixels.
    pub fn size(self, width: f64, height: f64) -> Self {
        call!(self.lxb, lxb_app_size(self.raw.as_ptr(), width, height) else ());
        self
    }

    /// Draw the whole window rather than the standard pane and flow.
    pub fn plain(self) -> Self {
        call!(self.lxb, lxb_app_plain(self.raw.as_ptr()) else ());
        self
    }

    /// Move the selection yourself: every action arrives at the page.
    pub fn driven(self) -> Self {
        call!(self.lxb, lxb_app_driven(self.raw.as_ptr()) else ());
        self
    }

    /// Open the window and draw `page` every frame until it closes. A panic
    /// in `page` closes the window and is resumed here, after the library
    /// has returned.
    pub fn run<F: FnMut(&Page)>(&mut self, page: F) -> Result<(), String> {
        let mut frame = Frame {
            page,
            panic: None,
            lxb: self.lxb,
        };
        let data = (&raw mut frame).cast::<c_void>();
        let code =
            call!(self.lxb, lxb_app_run(self.raw.as_ptr(), Some(trampoline::<F>), data) else 1);
        frame.finish(code, self)
    }

    /// One settled frame of `page`, written to a PNG at `path`, with no
    /// display at all (`lxb_app_shot`). `page` is called for two frames,
    /// the second after menus and dialogs opened in the first have arrived.
    pub fn shot<F: FnMut(&Page)>(
        &mut self,
        path: impl AsRef<Path>,
        width: u32,
        height: u32,
        seconds: f32,
        page: F,
    ) -> Result<(), String> {
        let mut frame = Frame {
            page,
            panic: None,
            lxb: self.lxb,
        };
        let path = path.as_ref();
        let c_path = c_string(&path.to_string_lossy());
        let data = (&raw mut frame).cast::<c_void>();
        let code = call!(self.lxb, lxb_app_shot(self.raw.as_ptr(), c_path.as_ptr(), width, height, seconds, Some(trampoline::<F>), data) else 1);
        frame.finish(code, self)
    }

    /// [`App::shot`] `frames` frames after the first, a sixtieth of a second
    /// apart, with whatever is moving caught where it has got to
    /// (`lxb_app_shot_after`; an older library takes the settled picture).
    pub fn shot_after<F: FnMut(&Page)>(
        &mut self,
        path: impl AsRef<Path>,
        width: u32,
        height: u32,
        seconds: f32,
        frames: u32,
        page: F,
    ) -> Result<(), String> {
        if self.lxb.sym.lxb_app_shot_after.is_none() {
            return self.shot(path, width, height, seconds, page);
        }
        let mut frame = Frame {
            page,
            panic: None,
            lxb: self.lxb,
        };
        let c_path = c_string(&path.as_ref().to_string_lossy());
        let data = (&raw mut frame).cast::<c_void>();
        let code = call!(self.lxb, lxb_app_shot_after(self.raw.as_ptr(), c_path.as_ptr(), width, height, seconds, frames, Some(trampoline::<F>), data) else 1);
        frame.finish(code, self)
    }

    /// Count the session as low-end hardware whatever the shell says.
    pub fn low_end(self) -> Self {
        call!(self.lxb, lxb_app_low_end(self.raw.as_ptr()) else ());
        self
    }

    /// A mark of the application's own, drawn by `name` wherever a mark of
    /// the toolkit's is (`lxb_app_mark`): an SVG shape saying `lxb:shape`.
    /// Before [`App::run`] or [`App::shot`].
    pub fn mark(&mut self, name: &str, svg: &[u8]) -> Result<(), String> {
        let Some(function) = self.lxb.sym.lxb_app_mark else {
            return Err("this liblxb_app.so cannot take marks of its own".into());
        };
        let name = c_string(name);
        // SAFETY: the application is alive; the bytes are read during the call.
        let added = unsafe {
            function(
                self.raw.as_ptr(),
                name.as_ptr(),
                svg.as_ptr().cast(),
                svg.len() as std::ffi::c_ulong,
            )
        };
        if added == 1 {
            Ok(())
        } else {
            Err(self
                .trouble()
                .unwrap_or_else(|| "the mark was refused".into()))
        }
    }

    /// Why the window could not open or the picture was not written.
    pub fn trouble(&self) -> Option<String> {
        let raw = call!(self.lxb, lxb_app_trouble(self.raw.as_ptr()) else std::ptr::null());
        if raw.is_null() {
            return None;
        }
        // SAFETY: a NUL-terminated string owned by the application.
        Some(
            unsafe { CStr::from_ptr(raw) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

impl Drop for App {
    fn drop(&mut self) {
        call!(self.lxb, lxb_app_free(self.raw.as_ptr()) else ());
    }
}

struct Frame<F> {
    page: F,
    panic: Option<Box<dyn Any + Send>>,
    lxb: &'static Lxb,
}

impl<F> Frame<F> {
    fn finish(self, code: i32, app: &App) -> Result<(), String> {
        if let Some(panic) = self.panic {
            resume_unwind(panic);
        }
        if code == 0 {
            Ok(())
        } else {
            Err(app
                .trouble()
                .unwrap_or_else(|| format!("the application stopped with {code}")))
        }
    }
}

extern "C" fn trampoline<F: FnMut(&Page)>(raw: *mut RawPage, data: *mut c_void) {
    // SAFETY: `data` is the `Frame<F>` `run`/`shot` handed over, alive and
    // not otherwise borrowed for the length of the library call.
    let Some(frame) = (unsafe { data.cast::<Frame<F>>().as_mut() }) else {
        return;
    };
    let Some(raw) = NonNull::new(raw) else {
        return;
    };
    let page = Page::new(raw, frame.lxb);
    if frame.panic.is_some() {
        page.quit();
        return;
    }
    let page_fn = &mut frame.page;
    if let Err(panic) = catch_unwind(AssertUnwindSafe(|| page_fn(&page))) {
        frame.panic = Some(panic);
        page.quit();
    }
}
