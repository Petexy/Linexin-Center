//! Development harness: loads the built widget library through the C ABI with a
//! stub host, shows it in a window and renders that window to PNG.
//!
//! ```sh
//! cargo build -p linexin-system-information
//! cargo run -p linexin-system-information --example harness -- --lang pl_PL --output out.png
//! ```
//! Run it inside a headless compositor (`WAYLAND_DISPLAY` set, `DISPLAY` unset).
//! `--bottom-output FILE` also renders the content scrolled to the end,
//! `--switch-output FILE` toggles light/dark afterwards and renders again,
//! `--hide-sidebar` creates it in single-widget mode and `--destroy-early`
//! drops the widget before its data arrives. Every run removes the widget at the
//! end and reports whether it was finalized and its style-manager handler gone.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::glib::translate::{FromGlibPtrFull, IntoGlib};
use gtk::{gio, glib, graphene};
use linexin_plugin_api::{
    ABI_VERSION, CreateContextV1, ENTRY_SYMBOL, EntryV1, Header, HostApiV1, WidgetDescriptorV1,
};
use regex::Regex;

const DEFAULT_LOCALIZATION: &str = "/usr/share/linexin/widgets/localization";

struct Options {
    library: PathBuf,
    localization: PathBuf,
    language: CString,
    dark: bool,
    width: i32,
    height: i32,
    output: Option<PathBuf>,
    bottom_output: Option<PathBuf>,
    switch_output: Option<PathBuf>,
    hide_sidebar: bool,
    destroy_early: bool,
}

static OPTIONS: OnceLock<Options> = OnceLock::new();

fn options() -> &'static Options {
    OPTIONS
        .get()
        .expect("options are set before the host is used")
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options {
        library: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/debug/liblinexin_system_information.so"),
        localization: PathBuf::from(DEFAULT_LOCALIZATION),
        language: c"en_US".into(),
        dark: false,
        width: 900,
        height: 1000,
        output: None,
        bottom_output: None,
        switch_output: None,
        hide_sidebar: false,
        destroy_early: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--library" => options.library = value()?.into(),
            "--localization" => options.localization = value()?.into(),
            "--lang" => options.language = CString::new(value()?).map_err(|e| e.to_string())?,
            "--dark" => options.dark = true,
            "--width" => options.width = value()?.parse().map_err(|_| "bad --width")?,
            "--height" => options.height = value()?.parse().map_err(|_| "bad --height")?,
            "--output" => options.output = Some(value()?.into()),
            "--bottom-output" => options.bottom_output = Some(value()?.into()),
            "--switch-output" => options.switch_output = Some(value()?.into()),
            "--hide-sidebar" => options.hide_sidebar = true,
            "--destroy-early" => options.destroy_early = true,
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    Ok(options)
}

/// `translations = {"English": "Translated", ...}` of one dictionary file.
fn load_dictionary(path: &Path) -> HashMap<String, String> {
    static ENTRY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#""((?:[^"\\]|\\.)*)"\s*:\s*"((?:[^"\\]|\\.)*)""#).expect("valid pattern")
    });
    let unescape = |text: &str| {
        text.replace("\\\"", "\"")
            .replace("\\n", "\n")
            .replace("\\\\", "\\")
    };
    let source = std::fs::read_to_string(path).unwrap_or_default();
    ENTRY
        .captures_iter(&source)
        .map(|c| (unescape(&c[1]), unescape(&c[2])))
        .collect()
}

fn lookup(domain: &str, text: &str) -> String {
    static CACHE: LazyLock<Mutex<HashMap<PathBuf, HashMap<String, String>>>> =
        LazyLock::new(Mutex::default);
    let options = options();
    let language = options.language.to_string_lossy();
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for language in [language.as_ref(), "en_US"] {
        let path = options
            .localization
            .join(language)
            .join(format!("{domain}_dictionary.py"));
        let dictionary = cache
            .entry(path.clone())
            .or_insert_with(|| load_dictionary(&path));
        if let Some(translated) = dictionary.get(text) {
            return translated.clone();
        }
    }
    text.to_owned()
}

unsafe extern "C" fn language() -> *const c_char {
    options().language.as_ptr()
}

unsafe extern "C" fn translate(domain: *const c_char, text: *const c_char) -> *mut c_char {
    if domain.is_null() || text.is_null() {
        return std::ptr::null_mut();
    }
    let (domain, text) = unsafe { (CStr::from_ptr(domain), CStr::from_ptr(text)) };
    let translated = lookup(&domain.to_string_lossy(), &text.to_string_lossy());
    CString::new(translated).map_or(std::ptr::null_mut(), CString::into_raw)
}

unsafe extern "C" fn free_string(string: *mut c_char) {
    if !string.is_null() {
        drop(unsafe { CString::from_raw(string) });
    }
}

unsafe extern "C" fn translate_widget(_: *const c_char, _: *mut c_void) {}
unsafe extern "C" fn no_op() {}
unsafe extern "C" fn validate_password(_: *const c_char) -> bool {
    false
}
unsafe extern "C" fn set_password(_: *const c_char) {}
unsafe extern "C" fn has_password() -> bool {
    false
}
unsafe extern "C" fn wrapper_path() -> *const c_char {
    c"/bin/false".as_ptr()
}
unsafe extern "C" fn run_privileged(_: *const *const c_char) -> c_int {
    -1
}

unsafe extern "C" fn askpass_path() -> *const c_char {
    c"/bin/false".as_ptr()
}

unsafe extern "C" fn not_compact(_: *mut c_void) -> bool {
    false
}

static HOST: HostApiV1 = HostApiV1 {
    abi_version: ABI_VERSION,
    struct_size: size_of::<HostApiV1>() as u32,
    language,
    translate,
    free_string,
    translate_widget,
    lock_ui: no_op,
    unlock_ui: no_op,
    sudo_validate_password: validate_password,
    sudo_set_password: set_password,
    sudo_has_password: has_password,
    sudo_forget_password: no_op,
    sudo_wrapper_path: wrapper_path,
    sudo_start_session: no_op,
    sudo_stop_session: no_op,
    sudo_run_privileged: run_privileged,
    refresh_widgets: no_op,
    sudo_askpass_path: askpass_path,
    window_compact_mode: not_compact,
};

fn load_descriptor(path: &Path) -> Result<&'static WidgetDescriptorV1, String> {
    // SAFETY: a trusted development build of the widget.
    let library = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;
    // Like the Center, never unload: GTK keeps pointers into the library.
    let library = Box::leak(Box::new(library));
    let entry = unsafe { library.get::<EntryV1>(ENTRY_SYMBOL) }.map_err(|e| e.to_string())?;
    let descriptor = unsafe { entry(&HOST) };
    if !unsafe { Header::is_compatible(descriptor.cast(), size_of::<WidgetDescriptorV1>()) } {
        return Err("incompatible descriptor".into());
    }
    Ok(unsafe { &*descriptor })
}

fn find<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Option<T> {
    widget.clone().downcast::<T>().ok().or_else(|| {
        std::iter::successors(widget.first_child(), |child| child.next_sibling())
            .find_map(|child| find(&child))
    })
}

fn render(widget: &impl IsA<gtk::Widget>, path: &Path) -> Result<(), String> {
    let widget = widget.as_ref();
    let (width, height) = (widget.width(), widget.height());
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(widget)).snapshot(&snapshot, width.into(), height.into());
    let node = snapshot.to_node().ok_or("nothing was drawn")?;
    let renderer = widget
        .native()
        .and_then(|n| n.renderer())
        .ok_or("no renderer")?;
    let viewport = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
    let texture = renderer.render_texture(&node, Some(&viewport));
    texture.save_to_png(path).map_err(|e| e.to_string())?;
    println!("rendered {}x{height} to {}", width, path.display());
    Ok(())
}

/// Number of `notify::dark` handlers connected to the style manager.
fn dark_handlers() -> u32 {
    use glib::gobject_ffi::{
        G_SIGNAL_MATCH_DETAIL, G_SIGNAL_MATCH_ID, g_signal_handlers_block_matched,
        g_signal_handlers_unblock_matched,
    };
    let manager = adw::StyleManager::default();
    let instance = manager.as_ptr().cast();
    let notify = glib::subclass::SignalId::lookup("notify", glib::Object::static_type())
        .expect("GObject has a notify signal")
        .into_glib();
    let detail = glib::Quark::from_str("dark").into_glib();
    let mask = G_SIGNAL_MATCH_ID | G_SIGNAL_MATCH_DETAIL;
    let (closure, null) = (std::ptr::null_mut(), std::ptr::null_mut());
    // Blocking is counted, so unblocking the same handlers restores their state.
    unsafe {
        let count =
            g_signal_handlers_block_matched(instance, mask, notify, detail, closure, null, null);
        g_signal_handlers_unblock_matched(instance, mask, notify, detail, closure, null, null);
        count
    }
}

fn scheme(dark: bool) -> adw::ColorScheme {
    if dark {
        adw::ColorScheme::ForceDark
    } else {
        adw::ColorScheme::ForceLight
    }
}

fn activate(app: &adw::Application) -> Result<(), String> {
    let options = options();
    adw::StyleManager::default().set_color_scheme(scheme(options.dark));
    // The Center's own rule for dimmed labels.
    let css = gtk::CssProvider::new();
    css.load_from_string(".dim-label { opacity: 0.6; }");
    let display = gtk::gdk::Display::default().ok_or("no display")?;
    gtk::style_context_add_provider_for_display(
        &display,
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let descriptor = load_descriptor(&options.library)?;
    let text = |pointer| {
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    };
    let (id, name) = (text(descriptor.widget_id), text(descriptor.name));
    println!(
        "widget_id={id} name={name:?} sidebar={:?} icon={}",
        lookup(&id, &name),
        text(descriptor.icon)
    );

    let window = adw::ApplicationWindow::new(app);
    window.set_default_size(options.width, options.height);
    let context = CreateContextV1 {
        abi_version: ABI_VERSION,
        struct_size: size_of::<CreateContextV1>() as u32,
        host: &HOST,
        window: window.upcast_ref::<gtk::Window>().as_ptr().cast(),
        hide_sidebar: options.hide_sidebar,
        voice_autostart: false,
        conversation_id: std::ptr::null(),
    };
    let create = descriptor.create.ok_or("descriptor without create")?;
    println!("notify::dark handlers before create: {}", dark_handlers());
    let pointer = unsafe { create(&context) };
    if pointer.is_null() {
        return Err("create returned null".into());
    }
    let widget = unsafe { gtk::Widget::from_glib_full(pointer.cast::<gtk::ffi::GtkWidget>()) };
    window.set_content(Some(&widget));
    window.present();

    if options.destroy_early {
        let weak = widget.downgrade();
        window.set_content(None::<&gtk::Widget>);
        drop(widget);
        iterate(Duration::from_millis(300));
        println!(
            "widget finalized before its data arrived: {}, notify::dark handlers: {}",
            weak.upgrade().is_none(),
            dark_handlers()
        );
        // Outlive the worker thread to see that nothing touches the old widget.
        glib::timeout_add_local_once(Duration::from_secs(4), {
            let app = app.clone();
            move || app.quit()
        });
        return Ok(());
    }

    let started = Instant::now();
    let app = app.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        if find::<adw::ActionRow>(&widget).is_none() && started.elapsed() < Duration::from_secs(20)
        {
            return glib::ControlFlow::Continue;
        }
        println!("data shown after {:.2}s", started.elapsed().as_secs_f64());
        let (window, widget, app) = (window.clone(), widget.clone(), app.clone());
        glib::timeout_add_local_once(Duration::from_millis(500), move || {
            if let Err(error) = finish(&window, widget) {
                eprintln!("harness: {error}");
            }
            app.quit();
        });
        glib::ControlFlow::Break
    });
    Ok(())
}

/// Long enough for the overlay scrollbar shown by scrolling to fade out again,
/// so renders do not depend on animation timing.
const SETTLE: Duration = Duration::from_secs(3);

fn iterate(duration: Duration) {
    let context = glib::MainContext::default();
    let until = Instant::now() + duration;
    while Instant::now() < until {
        context.iteration(false);
    }
}

fn finish(window: &adw::ApplicationWindow, widget: gtk::Widget) -> Result<(), String> {
    let options = options();
    if let Some(path) = &options.output {
        render(window, path)?;
    }
    if let Some(path) = &options.bottom_output {
        let scrolled = find::<gtk::ScrolledWindow>(&widget).ok_or("no scrolled window")?;
        let adjustment = scrolled.vadjustment();
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
        iterate(SETTLE);
        render(window, path)?;
        adjustment.set_value(0.0);
    }
    if let Some(path) = &options.switch_output {
        adw::StyleManager::default().set_color_scheme(scheme(!options.dark));
        iterate(SETTLE);
        render(window, path)?;
    }
    if options.hide_sidebar {
        println!("window default size: {:?}", window.default_size());
    }
    println!("notify::dark handlers while shown: {}", dark_handlers());
    let weak = widget.downgrade();
    window.set_content(None::<&gtk::Widget>);
    drop(widget);
    // GTK drops its last internal references on the next frames.
    iterate(Duration::from_millis(300));
    println!(
        "widget finalized after removal: {}, notify::dark handlers: {}",
        weak.upgrade().is_none(),
        dark_handlers()
    );
    Ok(())
}

fn main() -> ExitCode {
    match parse_options() {
        Ok(options) => {
            let _ = OPTIONS.set(options);
        }
        Err(error) => {
            eprintln!("harness: {error}");
            return ExitCode::FAILURE;
        }
    }
    let app = adw::Application::builder()
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(|app| {
        if let Err(error) = activate(app) {
            eprintln!("harness: {error}");
            app.quit();
        }
    });
    if app.run_with_args::<&str>(&[]) == glib::ExitCode::SUCCESS {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
