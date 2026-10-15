//! The entry point fails and returns null.
mod api;

#[unsafe(no_mangle)]
pub extern "C" fn linexin_widget_v1(_: *const api::HostApiV1) -> *const api::WidgetDescriptorV1 {
    std::ptr::null()
}
