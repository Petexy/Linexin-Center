//! A descriptor without a `create` function.
mod api;

static DESCRIPTOR: api::WidgetDescriptorV1 = api::WidgetDescriptorV1 {
    create: None,
    ..api::descriptor(c"null_create".as_ptr(), c"Null create fixture".as_ptr())
};

#[unsafe(no_mangle)]
pub extern "C" fn linexin_widget_v1(_: *const api::HostApiV1) -> *const api::WidgetDescriptorV1 {
    &raw const DESCRIPTOR
}
