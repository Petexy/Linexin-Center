//! Hand-written mirror of the plugin-api v1 C layout, so fixtures build with
//! plain `rustc` (keep in sync with crates/plugin-api/src/lib.rs).
#![allow(dead_code)]

use std::ffi::{c_char, c_int, c_void};

#[repr(C)]
pub struct Header {
    pub abi_version: u32,
    pub struct_size: u32,
}

#[repr(C)]
pub struct HostApiV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub language: unsafe extern "C" fn() -> *const c_char,
    pub translate: unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_char,
    pub free_string: unsafe extern "C" fn(*mut c_char),
    pub translate_widget: unsafe extern "C" fn(*const c_char, *mut c_void),
    pub lock_ui: unsafe extern "C" fn(),
    pub unlock_ui: unsafe extern "C" fn(),
    pub sudo_validate_password: unsafe extern "C" fn(*const c_char) -> bool,
    pub sudo_set_password: unsafe extern "C" fn(*const c_char),
    pub sudo_has_password: unsafe extern "C" fn() -> bool,
    pub sudo_forget_password: unsafe extern "C" fn(),
    pub sudo_wrapper_path: unsafe extern "C" fn() -> *const c_char,
    pub sudo_start_session: unsafe extern "C" fn(),
    pub sudo_stop_session: unsafe extern "C" fn(),
    pub sudo_run_privileged: unsafe extern "C" fn(*const *const c_char) -> c_int,
}

#[repr(C)]
pub struct CreateContextV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub host: *const HostApiV1,
    pub window: *mut c_void,
    pub hide_sidebar: bool,
}

pub type Create = unsafe extern "C" fn(*const CreateContextV1) -> *mut c_void;
pub type WidgetCallback = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
pub type CloseRequest = unsafe extern "C" fn(*mut c_void);

#[repr(C)]
pub struct WidgetDescriptorV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub widget_id: *const c_char,
    pub name: *const c_char,
    pub icon: *const c_char,
    pub create: Option<Create>,
    pub header_bar_widget: Option<WidgetCallback>,
    pub close_request: Option<CloseRequest>,
}

unsafe impl Sync for WidgetDescriptorV1 {}

unsafe extern "C" fn never_called(_: *const CreateContextV1) -> *mut c_void {
    // Descriptors of rejected fixtures must never reach `create`.
    std::process::abort();
}

/// A valid descriptor whose `create` aborts; tests override single fields.
pub const fn descriptor(widget_id: *const c_char, name: *const c_char) -> WidgetDescriptorV1 {
    WidgetDescriptorV1 {
        abi_version: 1,
        struct_size: size_of::<WidgetDescriptorV1>() as u32,
        widget_id,
        name,
        icon: c"".as_ptr(),
        create: Some(never_called),
        header_bar_widget: None,
        close_request: None,
    }
}
