//! A working native widget: a centered label (like a minimal Python widget).
//!
//! Build with plain rustc, choosing the descriptor at compile time:
//!   FIXTURE_ID=fixture_alpha FIXTURE_NAME="Fixture Alpha" FIXTURE_ICON=/path.svg \
//!   FIXTURE_TEXT="Fixture Alpha content" rustc --edition=2024 --crate-type=cdylib \
//!   -l gtk-4 -l gobject-2.0 label_widget.rs -o a-fixture-alpha.so
//!
//! The label text goes through the host's `translate` (domain FIXTURE_ID). In
//! single-widget mode the header bar gets a "Fixture action" button. When
//! LINEXIN_FIXTURE_CLOSE_LOG is set, close requests append the id to that file.

mod api;

use std::ffi::{CStr, c_char, c_void};
use std::io::Write;
use std::sync::atomic::{AtomicPtr, Ordering};

unsafe extern "C" {
    fn gtk_label_new(text: *const c_char) -> *mut c_void;
    fn gtk_button_new_with_label(label: *const c_char) -> *mut c_void;
    fn g_object_ref_sink(object: *mut c_void) -> *mut c_void;
}

const fn c_str(bytes: &'static [u8]) -> &'static CStr {
    match CStr::from_bytes_with_nul(bytes) {
        Ok(text) => text,
        Err(_) => panic!("fixture strings must not contain NUL"),
    }
}

const ID: &CStr = c_str(concat!(env!("FIXTURE_ID"), "\0").as_bytes());
const NAME: &CStr = c_str(concat!(env!("FIXTURE_NAME"), "\0").as_bytes());
const ICON: &CStr = c_str(concat!(env!("FIXTURE_ICON"), "\0").as_bytes());
const TEXT: &CStr = c_str(concat!(env!("FIXTURE_TEXT"), "\0").as_bytes());

static DESCRIPTOR: api::WidgetDescriptorV1 = api::WidgetDescriptorV1 {
    icon: ICON.as_ptr(),
    create: Some(create),
    header_bar_widget: Some(header_bar_widget),
    close_request: Some(close_request),
    ..api::descriptor(ID.as_ptr(), NAME.as_ptr())
};

static HOST: AtomicPtr<api::HostApiV1> = AtomicPtr::new(std::ptr::null_mut());

#[unsafe(no_mangle)]
pub unsafe extern "C" fn linexin_widget_v1(
    host: *const api::HostApiV1,
) -> *const api::WidgetDescriptorV1 {
    let header = host.cast::<api::Header>();
    if host.is_null()
        || unsafe { (*header).abi_version } != 1
        || (unsafe { (*header).struct_size } as usize) < size_of::<api::HostApiV1>()
    {
        return std::ptr::null();
    }
    HOST.store(host.cast_mut(), Ordering::Release);
    &raw const DESCRIPTOR
}

unsafe extern "C" fn create(context: *const api::CreateContextV1) -> *mut c_void {
    if context.is_null() || unsafe { (*context).window }.is_null() {
        return std::ptr::null_mut();
    }
    let host = HOST.load(Ordering::Acquire);
    unsafe {
        let translated = ((*host).translate)(ID.as_ptr(), TEXT.as_ptr());
        let label = gtk_label_new(if translated.is_null() {
            TEXT.as_ptr()
        } else {
            translated
        });
        ((*host).free_string)(translated);
        g_object_ref_sink(label)
    }
}

unsafe extern "C" fn header_bar_widget(_widget: *mut c_void) -> *mut c_void {
    unsafe { g_object_ref_sink(gtk_button_new_with_label(c"Fixture action".as_ptr())) }
}

unsafe extern "C" fn close_request(_widget: *mut c_void) {
    if let Some(path) = std::env::var_os("LINEXIN_FIXTURE_CLOSE_LOG") {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{}", ID.to_string_lossy());
        }
    }
}
