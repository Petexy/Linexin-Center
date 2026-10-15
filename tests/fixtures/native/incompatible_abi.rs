//! Reports ABI version 77: the host must reject it after reading only the header.
mod api;

static HEADER: api::Header = api::Header {
    abi_version: 77,
    struct_size: size_of::<api::WidgetDescriptorV1>() as u32,
};

#[unsafe(no_mangle)]
pub extern "C" fn linexin_widget_v1(_: *const api::HostApiV1) -> *const api::Header {
    &raw const HEADER
}
