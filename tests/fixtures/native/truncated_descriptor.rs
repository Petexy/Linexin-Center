//! Version 1, but only a header: the host must not read past `struct_size`.
mod api;

static HEADER: api::Header = api::Header {
    abi_version: 1,
    struct_size: size_of::<api::Header>() as u32,
};

#[unsafe(no_mangle)]
pub extern "C" fn linexin_widget_v1(_: *const api::HostApiV1) -> *const api::Header {
    &raw const HEADER
}
