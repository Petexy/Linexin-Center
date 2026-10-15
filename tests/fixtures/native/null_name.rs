//! A descriptor whose `name` string is null.
mod api;

static DESCRIPTOR: api::WidgetDescriptorV1 =
    api::descriptor(c"null_name".as_ptr(), std::ptr::null());

#[unsafe(no_mangle)]
pub extern "C" fn linexin_widget_v1(_: *const api::HostApiV1) -> *const api::WidgetDescriptorV1 {
    &raw const DESCRIPTOR
}
