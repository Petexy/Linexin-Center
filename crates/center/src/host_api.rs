//! The [`HostApiV1`] services handed to native widgets.

use gtk::glib::translate::{Borrowed, FromGlibPtrBorrow};
use linexin_localization::{global, widgets};
use linexin_plugin_api::{ABI_VERSION, HostApiV1};
use linexin_services::{lock, sudo};
use std::ffi::{CStr, CString, OsStr, c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

pub static HOST_API: HostApiV1 = HostApiV1 {
    abi_version: ABI_VERSION,
    struct_size: size_of::<HostApiV1>() as u32,
    language,
    translate,
    free_string,
    translate_widget,
    lock_ui,
    unlock_ui,
    sudo_validate_password,
    sudo_set_password,
    sudo_has_password,
    sudo_forget_password,
    sudo_wrapper_path,
    sudo_start_session,
    sudo_stop_session,
    sudo_run_privileged,
    refresh_widgets,
    sudo_askpass_path,
    window_compact_mode,
};

/// Set by [`HostApiV1::refresh_widgets`]; the LineXinBar front end polls it
/// each frame, the GTK windows are told on their main loop.
static REFRESH_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Whether a widget asked for a rescan since the last call.
pub fn take_refresh_request() -> bool {
    REFRESH_REQUESTED.swap(false, Ordering::SeqCst)
}

/// Run `f`, turning a panic into `fallback` (panics must not cross the C ABI).
fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// Borrow a NUL-terminated argument as text; `None` for null.
unsafe fn text<'a>(pointer: *const c_char) -> Option<std::borrow::Cow<'a, str>> {
    // SAFETY: the ABI requires null or a valid NUL-terminated string.
    (!pointer.is_null()).then(|| unsafe { CStr::from_ptr(pointer) }.to_string_lossy())
}

fn process_string(
    cell: &'static OnceLock<CString>,
    value: impl FnOnce() -> Vec<u8>,
) -> *const c_char {
    cell.get_or_init(|| {
        let mut bytes = value();
        bytes.retain(|&b| b != 0);
        CString::new(bytes).unwrap_or_default()
    })
    .as_ptr()
}

unsafe extern "C" fn language() -> *const c_char {
    static LANGUAGE: OnceLock<CString> = OnceLock::new();
    guard(c"en_US".as_ptr(), || {
        process_string(&LANGUAGE, || global().language().as_bytes().to_vec())
    })
}

unsafe extern "C" fn translate(domain: *const c_char, text_pointer: *const c_char) -> *mut c_char {
    guard(std::ptr::null_mut(), || {
        let (Some(domain), Some(source)) = (unsafe { text(domain) }, unsafe { text(text_pointer) })
        else {
            return std::ptr::null_mut();
        };
        let translated = global().translate(&source, &domain);
        CString::new(translated)
            .or_else(|_| CString::new(source.as_bytes()))
            .unwrap_or_default()
            .into_raw()
    })
}

unsafe extern "C" fn free_string(string: *mut c_char) {
    if !string.is_null() {
        // SAFETY: only pointers produced by `translate` (CString::into_raw) are passed in.
        guard((), || drop(unsafe { CString::from_raw(string) }));
    }
}

unsafe extern "C" fn translate_widget(domain: *const c_char, widget: *mut c_void) {
    guard((), || {
        let Some(domain) = (unsafe { text(domain) }) else {
            return;
        };
        if widget.is_null() || !gtk::is_initialized_main_thread() {
            return;
        }
        // SAFETY: the caller passes a live GObject instance (ABI contract).
        let is_widget = unsafe {
            gtk::glib::gobject_ffi::g_type_check_instance_is_a(
                widget.cast(),
                gtk::ffi::gtk_widget_get_type(),
            )
        } != 0;
        if is_widget {
            // SAFETY: checked above to be a GtkWidget. Borrow it without touching
            // the reference count: from_glib_none would sink a floating widget the
            // plugin has not parented yet, and dropping it would then free it.
            let widget: Borrowed<gtk::Widget> =
                unsafe { gtk::Widget::from_glib_borrow(widget.cast::<gtk::ffi::GtkWidget>()) };
            widgets::translate_dialog(&widget, &domain);
        }
    })
}

unsafe extern "C" fn lock_ui() {
    guard((), lock::lock)
}

unsafe extern "C" fn unlock_ui() {
    guard((), lock::unlock)
}

unsafe extern "C" fn sudo_validate_password(password: *const c_char) -> bool {
    guard(false, || {
        unsafe { text(password) }
            .is_some_and(|password| sudo::manager().validate_password(&password))
    })
}

unsafe extern "C" fn sudo_set_password(password: *const c_char) {
    guard((), || {
        if let Some(password) = unsafe { text(password) } {
            sudo::manager().set_password(&password);
        }
    })
}

unsafe extern "C" fn sudo_has_password() -> bool {
    guard(false, || sudo::manager().has_password())
}

unsafe extern "C" fn sudo_forget_password() {
    guard((), || sudo::manager().forget_password())
}

unsafe extern "C" fn sudo_wrapper_path() -> *const c_char {
    static WRAPPER: OnceLock<CString> = OnceLock::new();
    guard(c"".as_ptr(), || {
        process_string(&WRAPPER, || {
            sudo::manager()
                .wrapper_path()
                .as_os_str()
                .as_bytes()
                .to_vec()
        })
    })
}

unsafe extern "C" fn sudo_askpass_path() -> *const c_char {
    static ASKPASS: OnceLock<CString> = OnceLock::new();
    guard(c"".as_ptr(), || {
        process_string(&ASKPASS, || {
            sudo::manager()
                .askpass_path()
                .as_os_str()
                .as_bytes()
                .to_vec()
        })
    })
}

unsafe extern "C" fn sudo_start_session() {
    guard((), || sudo::manager().start_session())
}

unsafe extern "C" fn sudo_stop_session() {
    guard((), || sudo::manager().stop_session())
}

unsafe extern "C" fn window_compact_mode(window: *mut c_void) -> bool {
    guard(false, || {
        gtk::is_initialized_main_thread() && crate::window::compact_mode_of(window.cast())
    })
}

unsafe extern "C" fn refresh_widgets() {
    guard((), || {
        REFRESH_REQUESTED.store(true, Ordering::SeqCst);
        if gtk::is_initialized() {
            gtk::glib::MainContext::default().invoke(|| {
                if take_refresh_request() {
                    crate::window::refresh_all();
                }
            });
        }
    })
}

unsafe extern "C" fn sudo_run_privileged(argv: *const *const c_char) -> c_int {
    guard(-1, || {
        if argv.is_null() {
            return -1;
        }
        let mut args = vec![];
        for index in 0.. {
            // SAFETY: the ABI requires a NULL-terminated array of C strings.
            let arg = unsafe { *argv.add(index) };
            if arg.is_null() {
                break;
            }
            args.push(OsStr::from_bytes(unsafe { CStr::from_ptr(arg) }.to_bytes()));
        }
        if args.is_empty() {
            return -1;
        }
        match sudo::manager().run_privileged(&args) {
            Ok(status) => status.code().unwrap_or(-1),
            Err(error) => {
                eprintln!("Privileged command failed: {error}");
                -1
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_strings() {
        assert_eq!(HOST_API.abi_version, ABI_VERSION);
        assert_eq!(HOST_API.struct_size as usize, size_of::<HostApiV1>());
        unsafe {
            assert!(!(HOST_API.language)().is_null());
            let result = (HOST_API.translate)(c"widget_loader".as_ptr(), c"Some text".as_ptr());
            assert_eq!(CStr::from_ptr(result).to_str(), Ok("Some text"));
            (HOST_API.free_string)(result);
            (HOST_API.free_string)(std::ptr::null_mut());
            assert!((HOST_API.translate)(std::ptr::null(), c"x".as_ptr()).is_null());
            assert_eq!((HOST_API.sudo_run_privileged)(std::ptr::null()), -1);
            let empty = [std::ptr::null::<c_char>()];
            assert_eq!((HOST_API.sudo_run_privileged)(empty.as_ptr()), -1);
        }
    }
}
