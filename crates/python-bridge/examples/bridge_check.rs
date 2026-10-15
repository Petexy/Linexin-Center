//! Headless end-to-end check of the Python bridge: constructs the fixture
//! widgets of `tests/python/fixtures/bridge` and the installed System
//! Information widget inside a Rust GTK window, exercises the injected
//! helpers, the sudo facade (with a fake `sudo`) and the UI lock, renders the
//! System Information widget to PNG, and scans + imports (without
//! constructing) the other installed widgets.
//!
//! Never run it on a desktop session: `examples/run_bridge_check.sh` starts a
//! private compositor and puts a fake `sudo` first in `PATH`.

use std::cell::RefCell;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};
use linexin_localization as localization;
use linexin_python_bridge::{LoadedWidget, PythonRuntime, WidgetArgs, WindowHooks};
use pyo3::prelude::*;
use pyo3::types::PyDict;

const INSTALLED: &str = "/usr/share/linexin/widgets";
const SYSTEM_INFORMATION: &str = "/usr/share/linexin/widgets/z-system-information-widget.py";

#[derive(Default)]
struct Check {
    failures: Vec<String>,
}

impl Check {
    fn expect(&mut self, passed: bool, what: impl AsRef<str>) {
        let what = what.as_ref();
        if passed {
            println!("PASS {what}");
        } else {
            println!("FAIL {what}");
            self.failures.push(what.to_owned());
        }
    }

    fn equal(&mut self, actual: impl AsRef<str>, expected: impl AsRef<str>, what: &str) {
        let (actual, expected) = (actual.as_ref(), expected.as_ref());
        self.expect(
            actual == expected,
            format!("{what}: {actual:?} (expected {expected:?})"),
        );
    }
}

fn main() -> ExitCode {
    let Some(fake_sudo) = std::env::var_os("LINEXIN_FAKE_SUDO_DIR").map(PathBuf::from) else {
        eprintln!("Run through examples/run_bridge_check.sh (private compositor, fake sudo).");
        return ExitCode::from(2);
    };
    if first_in_path("sudo").as_deref() != Some(fake_sudo.join("sudo").as_path()) {
        eprintln!("Refusing to run: the first `sudo` in PATH is not the fake one.");
        return ExitCode::from(2);
    }
    let output = std::env::var_os("LINEXIN_BRIDGE_CHECK_OUTPUT")
        .map_or_else(|| PathBuf::from("bridge-check"), PathBuf::from);
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/python/fixtures/bridge");

    let app = adw::Application::new(
        Some("io.github.linexin.BridgeCheck"),
        gio::ApplicationFlags::NON_UNIQUE,
    );
    let failures = Rc::new(RefCell::new(Vec::new()));
    let results = Rc::clone(&failures);
    app.connect_activate(move |app| {
        let mut check = Check::default();
        run(app, &fixtures, &output, &mut check);
        *results.borrow_mut() = check.failures;
        app.quit();
    });
    app.run_with_args::<&str>(&[]);
    let failures = failures.borrow();
    if failures.is_empty() {
        println!("bridge check: all passed");
        ExitCode::SUCCESS
    } else {
        println!("bridge check: {} failure(s)", failures.len());
        ExitCode::FAILURE
    }
}

fn first_in_path(program: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

fn run(app: &adw::Application, fixtures: &Path, output: &Path, check: &mut Check) {
    let runtime = match PythonRuntime::new() {
        Ok(runtime) => runtime,
        Err(error) => return check.expect(false, format!("Python runtime: {error}")),
    };
    let evaluator = runtime.dictionary_evaluator();
    let language = localization::detect_language(std::env::var("LINEXIN_LANG").ok().as_deref());
    let bases = [
        fixtures.join("localization"),
        Path::new(INSTALLED).join("localization"),
    ];
    localization::install(localization::Localization::load(
        &bases,
        &language,
        Some(&*evaluator),
    ));
    let polish = language == "pl_PL" && localization::global().has_domain("gettext_widget");
    println!("language {language}; fixture translations available: {polish}");

    let lock_events = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&lock_events);
    linexin_services::lock::set_apply_callback(move |locked| recorded.borrow_mut().push(locked));

    let application_window = adw::ApplicationWindow::new(app);
    application_window.set_title(Some("Linexin Center"));
    application_window.set_default_size(1280, 730);
    let window = application_window.clone().upcast::<gtk::Window>();
    let refreshes = Rc::new(RefCell::new(0));
    let refreshed = Rc::clone(&refreshes);
    runtime.install_window(
        &window,
        false,
        WindowHooks {
            refresh_widgets: Box::new(move || *refreshed.borrow_mut() += 1),
        },
    );
    let args = WidgetArgs {
        hide_sidebar: false,
        voice_autostart: true,
        conversation_id: Some("conversation-1".into()),
    };

    let gettext = single(
        check,
        &runtime,
        &fixtures.join("a-gettext_widget.py"),
        &window,
        &args,
    );
    if let Some(widget) = &gettext {
        check_gettext_widget(check, widget, polish, &lock_events);
    }
    let kwargs = single(
        check,
        &runtime,
        &fixtures.join("b-kwargs_widget.py"),
        &window,
        &args,
    );
    if let Some(widget) = &kwargs {
        check.equal(
            python(&widget.widget, "(w.received['voice_autostart'], w.received['conversation_id'], w.received['hide_sidebar'])"),
            "(True, 'conversation-1', False)",
            "all arguments reach a constructor that accepts them",
        );
        check.equal(
            python(&widget.widget, "w.received['window'].get_title()"),
            "Linexin Center",
            "window argument is the host window",
        );
        let header = runtime.header_bar_widget(widget);
        check.expect(
            header
                .and_downcast::<gtk::Button>()
                .and_then(|button| button.label())
                .as_deref()
                == Some("Header"),
            "get_header_bar_widget",
        );
        runtime.reactivate_voice(widget);
        runtime.reactivate_voice(widget);
        check.equal(
            python(&widget.widget, "w.toggles"),
            "[True, False, True]",
            "SIGUSR1 voice re-toggle",
        );
    }
    let legacy = single(
        check,
        &runtime,
        &fixtures.join("c-legacy_args_widget.py"),
        &window,
        &args,
    );
    if let (Some(widget), Some(kwargs)) = (&legacy, &kwargs) {
        check.equal(
            python(&widget.widget, "(w.hide_sidebar, w.window.hide_sidebar)"),
            "(False, False)",
            "(hide_sidebar, window) constructor",
        );
        check.equal(
            python(&kwargs.widget, "id(w.received['window'])"),
            python(&widget.widget, "id(w.window)"),
            "every widget receives the same window wrapper",
        );
        check.equal(
            icon(widget),
            "/usr/share/icons/fixture-legacy.png",
            "widgeticon",
        );
        python(&widget.widget, "w.refresh_host()");
        check.equal(
            refreshes.borrow().to_string(),
            "1",
            "window.refresh_widgets() on the main thread",
        );
        python(
            &widget.widget,
            "__import__('threading').Thread(target=w.refresh_host).start()",
        );
        let refreshed = wait_until(Duration::from_secs(5), || *refreshes.borrow() == 2);
        check.expect(
            refreshed,
            "window.refresh_widgets() from a worker thread runs on the main thread",
        );
        runtime.set_active_widget_id(&window, Some("fixture_legacy_args"));
        check.equal(
            python(&widget.widget, "w.window.active_widget_id"),
            "fixture_legacy_args",
            "active_widget_id",
        );
        check.equal(
            python(
                &widget.widget,
                "(w.window.resize_window(900, 700), tuple(w.window.get_default_size()))",
            ),
            "(True, (900, 700))",
            "resize_window",
        );
        window.set_size_request(-1, -1);
        window.set_default_size(1280, 730);
        runtime.close_request();
        check.equal(
            python(&widget.widget, "w.cleaned_up"),
            "True",
            "close request cleanup hook",
        );
    }
    let no_args = single(
        check,
        &runtime,
        &fixtures.join("d-no_args_widget.py"),
        &window,
        &args,
    );
    if let Some(widget) = &no_args {
        check.equal(
            python(&widget.widget, "w.constructed"),
            "['NoArgsWidget']",
            "only the widget class is constructed",
        );
        check.equal(&widget.widget_id, "fixture_no_args", "derived widget_id");
    }

    check_background_import(
        check,
        &runtime,
        &fixtures.join("e-slow_widget.py"),
        &window,
        &args,
    );

    match runtime.import_module(&fixtures.join("f-failing_widget.py")) {
        Ok(module) => {
            let results = runtime.instantiate(&module, &window, &args);
            check.expect(
                matches!(results.as_slice(), [Err(error)] if error.contains("RuntimeError: fixture constructor failure")),
                "constructor exceptions become errors with the traceback",
            );
        }
        Err(error) => check.expect(false, format!("import f-failing_widget: {error}")),
    }
    check.expect(
        runtime
            .import_module(&fixtures.join("g-import_error.py"))
            .is_err_and(|error| error.contains("ZeroDivisionError")),
        "import errors carry the traceback",
    );
    match runtime.import_module(&fixtures.join("h-dynamic_name_widget.py")) {
        Ok(module) => {
            let results = runtime.instantiate(&module, &window, &args);
            let summary: Vec<String> = results
                .iter()
                .map(|result| match result {
                    Ok(widget) => format!("{}={}", widget.class_name, widget.widget_name),
                    Err(error) => error.lines().last().unwrap_or_default().to_owned(),
                })
                .collect();
            check.equal(
                summary.join("; "),
                "DynamicWidget=Fixture Dynamic; PlainHelper in h-dynamic_name_widget.py has no widgetname",
                "fallback to every Gtk.Widget subclass except windows",
            );
        }
        Err(error) => check.expect(false, format!("import h-dynamic_name_widget: {error}")),
    }

    check_compact_voice(
        check,
        &runtime,
        app,
        &fixtures.join("compact_voice.py"),
        polish,
    );
    check_system_information(check, &runtime, &application_window, output, &language);
    check_installed_imports(&runtime);

    let wrapper =
        linexin_services::sudo::existing().map(|manager| manager.wrapper_path().to_owned());
    runtime.shutdown();
    runtime.shutdown();
    check.expect(
        wrapper.is_some_and(|wrapper| !wrapper.exists()),
        "shutdown removes the sudo helper files",
    );
    window.destroy();
}

/// Import and instantiate a module expected to hold exactly one widget.
fn single(
    check: &mut Check,
    runtime: &PythonRuntime,
    path: &Path,
    window: &gtk::Window,
    args: &WidgetArgs,
) -> Option<LoadedWidget> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let module = match runtime.import_module(path) {
        Ok(module) => module,
        Err(error) => {
            check.expect(false, format!("import {name}: {error}"));
            return None;
        }
    };
    let mut results = runtime.instantiate(&module, window, args);
    match (results.len(), results.pop()) {
        (1, Some(Ok(widget))) => {
            check.expect(
                true,
                format!("{name}: {} ({})", widget.widget_name, widget.class_name),
            );
            Some(widget)
        }
        (count, last) => {
            let error = last.and_then(Result::err).unwrap_or_default();
            check.expect(false, format!("{name}: {count} result(s) {error}"));
            None
        }
    }
}

fn check_gettext_widget(
    check: &mut Check,
    widget: &LoadedWidget,
    polish: bool,
    lock_events: &RefCell<Vec<bool>>,
) {
    let domain = localization::global().domain_for_module("a-gettext_widget");
    check.equal(
        python(&widget.widget, "w.underscore_domain()"),
        &domain,
        "_ is bound to the module's dictionary domain",
    );
    check.equal(
        python(&widget.widget, "w.forwarded"),
        "{}",
        "TypeError retry with (hide_sidebar, window)",
    );
    check.equal(
        python(&widget.widget, "w.window.get_title()"),
        "Linexin Center",
        "window argument",
    );
    check.equal(
        icon(widget),
        "/usr/share/icons/fixture-gettext.svg",
        "indirect widgeticon",
    );
    check.equal(&widget.widget_id, "fixture_gettext", "widget_id attribute");
    check.equal(
        python(
            &widget.widget,
            "type(__import__('sys').modules['_linexin_bridge'].sudo_manager).__name__",
        ),
        "SudoManager",
        "sudo_manager facade",
    );
    check.equal(
        python(&widget.widget, "type(w).run_wrapper.__globals__['sudo_manager'] is __import__('_linexin_bridge').sudo_manager"),
        "True",
        "sudo_manager injected into the module",
    );
    let heading = python(
        &widget.widget,
        "(lambda d: (d.get_heading(), d.get_body()))(w.make_dialog())",
    );
    if polish {
        check.equal(
            &widget.widget_name,
            "Widżet Gettext",
            "widgetname through the injected _",
        );
        check.equal(
            python(&widget.widget, "w.greeting"),
            "Cześć",
            "_() at construction",
        );
        check.equal(heading, "('Cześć', 'Świat')", "translate_dialog(dialog)");
    } else {
        check.equal(
            &widget.widget_name,
            "Fixture Gettext",
            "widgetname (no dictionaries)",
        );
        check.equal(
            heading,
            "('Hello', 'World')",
            "translate_dialog(dialog) without dictionaries",
        );
    }

    python(
        &widget.widget,
        "__import__('_linexin_bridge').sudo_manager.set_password('correct horse')",
    );
    lock_events.borrow_mut().clear();
    check.equal(
        python(&widget.widget, "w.run_wrapper('sh', '-c', 'exit 6')"),
        "6",
        "subprocess.run([wrapper, ...]) in a session",
    );
    let unlocked = wait_until(Duration::from_secs(5), || {
        !linexin_services::lock::is_locked() && lock_events.borrow().last() == Some(&false)
    });
    check.expect(
        unlocked && lock_events.borrow().first() == Some(&true),
        format!("lock callbacks {:?}", lock_events.borrow()),
    );
    lock_events.borrow_mut().clear();
    check.equal(
        python(&widget.widget, "w.run_privileged('sh', '-c', 'exit 7')"),
        "7",
        "sudo_manager.run_privileged",
    );
    let unlocked = wait_until(Duration::from_secs(5), || {
        lock_events.borrow().as_slice() == [true, false]
    });
    check.expect(
        unlocked,
        format!("run_privileged lock callbacks {:?}", lock_events.borrow()),
    );
    python(
        &widget.widget,
        "__import__('_linexin_bridge').sudo_manager.forget_password()",
    );
}

fn check_background_import(
    check: &mut Check,
    runtime: &PythonRuntime,
    path: &Path,
    window: &gtk::Window,
    args: &WidgetArgs,
) {
    let ticks = Rc::new(RefCell::new(0u32));
    let counter = Rc::clone(&ticks);
    let timer = glib::timeout_add_local(Duration::from_millis(20), move || {
        *counter.borrow_mut() += 1;
        glib::ControlFlow::Continue
    });
    let result = Rc::new(RefCell::new(None));
    let slot = Rc::clone(&result);
    let started = Instant::now();
    runtime.import_in_background(path.to_owned(), move |module| {
        *slot.borrow_mut() = Some(module)
    });
    let finished = wait_until(Duration::from_secs(20), || result.borrow().is_some());
    timer.remove();
    let elapsed = started.elapsed();
    check.expect(
        finished && *ticks.borrow() >= 40,
        format!(
            "background import ({elapsed:.2?}) while the main loop ran {} ticks",
            ticks.borrow()
        ),
    );
    if let Some(Ok(module)) = result.borrow_mut().take() {
        let results = runtime.instantiate(&module, window, args);
        check.expect(
            matches!(results.as_slice(), [Ok(widget)] if widget.widget_name == "Fixture Slow"),
            "instantiate a background-imported module",
        );
    }
}

fn check_compact_voice(
    check: &mut Check,
    runtime: &PythonRuntime,
    app: &adw::Application,
    path: &Path,
    polish: bool,
) {
    match runtime.launch_compact_voice(app, path, true) {
        Ok(window) => {
            check.equal(
                python(&window, "type(w).__module__"),
                "compact_voice",
                "module named after the file",
            );
            check.equal(
                python(&window, "w.voice_autostart"),
                "True",
                "voice_autostart",
            );
            let expected = if polish { "Cześć (głos)" } else { "Hello" };
            check.equal(
                python(
                    &window,
                    "type(w).__init__.__globals__['IMPORT_TIME_GREETING']",
                ),
                expected,
                "helpers injected before exec (compact voice)",
            );
            runtime.reactivate_compact_voice(&window);
            check.equal(
                python(&window, "w.toggles"),
                "[True]",
                "compact voice SIGUSR1 re-toggle",
            );
            check.expect(
                window.application().is_some(),
                "CompactVoiceWindow(application=...)",
            );
            window.destroy();
        }
        Err(error) => check.expect(false, format!("launch_compact_voice: {error}")),
    }
}

fn check_system_information(
    check: &mut Check,
    runtime: &PythonRuntime,
    window: &adw::ApplicationWindow,
    output: &Path,
    language: &str,
) {
    let path = Path::new(SYSTEM_INFORMATION);
    if !path.exists() {
        return println!("SKIP {SYSTEM_INFORMATION} is not installed");
    }
    let Some(widget) = single(
        check,
        runtime,
        path,
        window.upcast_ref(),
        &WidgetArgs::default(),
    ) else {
        return;
    };
    check.equal(
        icon(&widget),
        "/usr/share/icons/computer-linexin.svg",
        "System Information icon",
    );
    check.equal(
        &widget.widget_id,
        "system_information",
        "System Information widget_id",
    );
    window.set_content(Some(&widget.widget));
    window.present();
    let mapped = wait_until(Duration::from_secs(10), || {
        widget.widget.is_mapped() && widget.widget.height() > 0
    });
    check.expect(mapped, "System Information is shown in the Rust window");
    // Let its worker thread fill in the values.
    wait_until(Duration::from_millis(2500), || false);
    let labels = label_texts(&widget.widget);
    let expected = if language == "pl_PL" && localization::global().has_domain("system_information")
    {
        "Procesor"
    } else {
        "Processor"
    };
    check.expect(
        labels.iter().any(|label| label == expected),
        format!("System Information shows {expected:?}"),
    );
    for (name, target) in [
        ("widget", widget.widget.clone()),
        ("window", window.clone().upcast()),
    ] {
        let file = output.join(format!("system-information-{name}-{language}.png"));
        match render_png(&target, &file) {
            Ok(()) => println!(
                "RENDERED {} ({}x{})",
                file.display(),
                target.width(),
                target.height()
            ),
            Err(error) => check.expect(false, format!("render System Information {name}: {error}")),
        }
    }
    window.set_content(None::<&gtk::Widget>);
}

/// Scan + import only: constructing these would start their real work.
fn check_installed_imports(runtime: &PythonRuntime) {
    let Ok(entries) = std::fs::read_dir(INSTALLED) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "py"))
        .collect();
    paths.sort();
    for path in paths {
        let scanned = runtime.scan(&path);
        let started = Instant::now();
        let imported = runtime.import_module(&path);
        println!(
            "INSTALLED {} scan={:?} import={} ({:.2?})",
            path.file_name().unwrap_or_default().to_string_lossy(),
            scanned.map(|widget| (
                widget.widget_name,
                widget.icon_path,
                widget.widget_id,
                widget.class_name
            )),
            match &imported {
                Ok(_) => "ok".to_owned(),
                Err(error) => format!("error: {}", error.lines().last().unwrap_or_default()),
            },
            started.elapsed(),
        );
    }
}

fn icon(widget: &LoadedWidget) -> String {
    widget.icon_path.clone().unwrap_or_default()
}

/// `str(<expression>)` evaluated with `w` bound to the object's Python wrapper.
fn python(object: &impl IsA<glib::Object>, expression: &str) -> String {
    let address = object.upcast_ref::<glib::Object>().as_ptr() as usize;
    Python::attach(|py| -> PyResult<String> {
        let wrapper = py
            .import("gi._gi")?
            .getattr("pygobject_new_full")?
            .call1((address, false))?;
        let locals = PyDict::new(py);
        locals.set_item("w", wrapper)?;
        let code = CString::new(format!("str({expression})"))?;
        py.eval(&code, None, Some(&locals))?.extract()
    })
    .unwrap_or_else(|error| format!("ERROR {error}"))
}

fn wait_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + timeout;
    while !condition() {
        if Instant::now() >= deadline {
            return false;
        }
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

fn label_texts(widget: &gtk::Widget) -> Vec<String> {
    let mut texts = Vec::new();
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        texts.push(label.text().to_string());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        texts.extend(label_texts(&current));
        child = current.next_sibling();
    }
    texts
}

fn render_png(widget: &gtk::Widget, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(
        &snapshot,
        f64::from(widget.width()),
        f64::from(widget.height()),
    );
    let node = snapshot.to_node().ok_or("nothing to render")?;
    let renderer = widget
        .native()
        .and_then(|native| native.renderer())
        .ok_or("no renderer")?;
    let viewport =
        gtk::graphene::Rect::new(0.0, 0.0, widget.width() as f32, widget.height() as f32);
    renderer
        .render_texture(&node, Some(&viewport))
        .save_to_png(path)
        .map_err(|error| error.to_string())
}
