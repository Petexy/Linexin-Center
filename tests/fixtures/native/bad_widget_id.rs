//! A widget_id that is not usable as a translation domain / file name part.
mod api;

static DESCRIPTOR: api::WidgetDescriptorV1 =
    api::descriptor(c"../bad id".as_ptr(), c"Bad id fixture".as_ptr());

#[unsafe(no_mangle)]
pub extern "C" fn linexin_widget_v1(_: *const api::HostApiV1) -> *const api::WidgetDescriptorV1 {
    &raw const DESCRIPTOR
}
