//! A stand-in for LineXinBar's `liblxb_app.so` with only the functions the
//! Center cannot do without (`CORE` in `crates/lxb/src/sys.rs`), for
//! `crates/center/tests/lxb_fallback.rs`. `FAKE_LXB_APP` says how it fails:
//!
//! - `null`: `lxb_app_new` answers null;
//! - `no-window`: `lxb_app_run` fails before the first frame, as the real one
//!   does without a display or a graphics adapter;
//! - `after-frame`: it draws one frame, then fails, as when the display goes
//!   away.
//!
//! Built with `--cfg old` it lacks `lxb_app_run`, as a toolkit too old would:
//!   rustc --edition=2024 --crate-type=cdylib [--cfg old] fake_lxb_app.rs -o liblxb_app.so

use std::ffi::{c_char, c_float, c_void};

#[cfg(not(old))]
type PageFn = extern "C" fn(page: *mut c_void, data: *mut c_void);

fn mode() -> String {
    std::env::var("FAKE_LXB_APP").unwrap_or_default()
}

#[unsafe(no_mangle)]
pub extern "C" fn lxb_app_new(_app_id: *const c_char, _title: *const c_char) -> *mut c_void {
    if mode() == "null" {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(0_u64)).cast()
}

/// # Safety
///
/// `app` is null or what `lxb_app_new` answered, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lxb_app_free(app: *mut c_void) {
    if !app.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(app.cast::<u64>()) });
    }
}

#[cfg(not(old))]
#[unsafe(no_mangle)]
pub extern "C" fn lxb_app_run(
    _app: *mut c_void,
    page: Option<PageFn>,
    data: *mut c_void,
) -> std::ffi::c_int {
    if mode() == "after-frame"
        && let Some(page) = page
    {
        let mut fake_page = 0_u64;
        page((&raw mut fake_page).cast(), data);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn lxb_app_trouble(_app: *const c_void) -> *const c_char {
    match mode().as_str() {
        "after-frame" => c"the display went away (fake)".as_ptr(),
        _ => c"no window: no display (fake)".as_ptr(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn lxb_page_width(_page: *const c_void) -> c_float {
    1280.0
}

#[unsafe(no_mangle)]
pub extern "C" fn lxb_page_height(_page: *const c_void) -> c_float {
    800.0
}
