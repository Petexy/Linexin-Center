//! A shared library without the `linexin_widget_v1` entry point.
#[unsafe(no_mangle)]
pub extern "C" fn unrelated_function() -> u32 {
    1
}
