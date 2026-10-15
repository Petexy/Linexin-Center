//! Embedded CPython support for legacy Python widgets in
//! `/usr/share/linexin/widgets/*.py`, reproducing the original Center's module
//! loading contract (`_prepare_module`, `_extract_widgets_from_module`, …).
//!
//! The interpreter runs inside the Rust process; PyGObject wraps the very same
//! GTK objects the Rust host uses, so Python widgets live in the host window.
//! The GTK main loop runs without holding the GIL, so Python worker threads,
//! timeouts and signal handlers keep running as under the original Center.
//!
//! Host sequence, all on the GTK main thread: [`PythonRuntime::new`] after
//! `adw::init()` (its [`PythonRuntime::dictionary_evaluator`] feeds
//! `Localization::load`); [`PythonRuntime::install_window`] once the main
//! window exists; per widget file [`PythonRuntime::scan`], then
//! [`PythonRuntime::import_in_background`] (or [`PythonRuntime::import_module`]
//! when the scan found nothing) and [`PythonRuntime::instantiate`];
//! [`PythonRuntime::set_active_widget_id`] on selection,
//! [`PythonRuntime::close_request`] on close and [`PythonRuntime::shutdown`]
//! at exit. Widget constructors and Python callbacks may emit GTK signals
//! into host handlers while holding the GIL: such handlers must not block on
//! other threads.

mod host;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::{self, ThreadId};

use gtk::glib::{self, thread_guard::ThreadGuard, translate::from_glib_none};
use gtk::prelude::*;
use linexin_localization::Dictionary;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyCapsule, PyModule};

pub use linexin_localization::Evaluator;

/// Metadata read from a widget's source without importing it (original
/// `_scan_widget_metadata`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScannedWidget {
    /// `widgetname` as the instance will report it: a plain literal unchanged,
    /// a literal wrapped in `_()`/`gettext()` translated through the module's
    /// dictionary domain (as the injected `_` does during construction).
    pub widget_name: String,
    /// `widgeticon` (file path or themed icon name), preferring a file path when
    /// it is assigned indirectly (`self.widgeticon = self.some_path`).
    pub icon_path: Option<String>,
    /// `self.widget_id` literal, else `widget_name.lower().replace(' ', '_')`.
    pub widget_id: String,
    /// Name of the top-level class that assigns `widgetname`.
    pub class_name: String,
}

/// Constructor arguments the original passed to every widget class.
#[derive(Clone, Debug, Default)]
pub struct WidgetArgs {
    pub hide_sidebar: bool,
    pub voice_autostart: bool,
    pub conversation_id: Option<String>,
}

/// An imported (executed) widget module, not yet prepared/instantiated.
/// `Send`, so it can be produced on a worker thread.
pub struct Module {
    path: PathBuf,
    module: Py<PyAny>,
}

impl Module {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// A constructed Python widget. Keeps the Python instance alive.
pub struct LoadedWidget {
    /// The `Gtk.Widget` instance (same native object as on the Python side).
    pub widget: gtk::Widget,
    /// The instance's `widgetname` after construction (already passed through the
    /// injected `_`, i.e. possibly translated).
    pub widget_name: String,
    /// The instance's `widgeticon`, if any.
    pub icon_path: Option<String>,
    /// The instance's `widget_id` attribute, else derived from `widget_name`.
    pub widget_id: String,
    /// Class the instance was created from.
    pub class_name: String,
}

/// Rust callbacks exposed to Python widgets as attributes of the host window.
pub struct WindowHooks {
    /// `window.refresh_widgets()` — re-scan the widgets directory for new files.
    pub refresh_widgets: Box<dyn Fn()>,
}

pub struct PythonRuntime {
    bridge: Py<PyModule>,
}

const BRIDGE_SOURCE: &CStr = pyo3::ffi::c_str!(include_str!("../python/bridge.py"));

static BRIDGE: PyOnceLock<Py<PyModule>> = PyOnceLock::new();
static MAIN_THREAD: OnceLock<ThreadId> = OnceLock::new();

thread_local! {
    /// Hooks of installed windows, keyed by the `GtkWindow` address.
    static HOOKS: RefCell<HashMap<usize, Rc<WindowHooks>>> = RefCell::new(HashMap::new());
}

type ImportDone = Box<dyn FnOnce(Result<Module, String>)>;

/// Pre-initialize CPython the way the `python3` command does for the original
/// Center: PEP 538 C-locale coercion and PEP 540 UTF-8 mode in their automatic
/// settings (honoring `PYTHONUTF8`/`PYTHONCOERCECLOCALE`), so widgets get the
/// same stdio, filesystem and `open()` encodings. Without it the embedded
/// interpreter would use ASCII under a C/POSIX or not-generated locale.
///
/// Call first thing in `main`, before GTK or any other thread starts:
/// coercion may set `LC_CTYPE` in the environment.
pub fn preinitialize() {
    // SAFETY: plain C API calls before the interpreter exists; the config is
    // fully initialized by PyPreConfig_InitPythonConfig.
    unsafe {
        if pyo3::ffi::Py_IsInitialized() != 0 {
            return;
        }
        let mut config = std::mem::MaybeUninit::<pyo3::ffi::PyPreConfig>::uninit();
        pyo3::ffi::PyPreConfig_InitPythonConfig(config.as_mut_ptr());
        let status = pyo3::ffi::Py_PreInitialize(config.as_ptr());
        if pyo3::ffi::PyStatus_Exception(status) != 0 {
            eprintln!("Python pre-initialization failed; using the default configuration");
        }
    }
}

impl PythonRuntime {
    /// Initialize the interpreter (never writing bytecode into widget
    /// directories), register the host helper module, load the bridge, create
    /// the `sudo_manager` facade over [`linexin_services::sudo`], and install the
    /// original subprocess patches (`Popen`, `run`, `call`, `check_call`,
    /// `check_output`) that hold [`linexin_services::lock`] while a command whose
    /// executable is the sudo wrapper runs. Call on the GTK main thread.
    pub fn new() -> Result<Self, String> {
        let _ = MAIN_THREAD.set(thread::current().id());
        Python::attach(|py| {
            BRIDGE
                .get_or_try_init(py, || load_bridge(py))
                .map(|bridge| Self {
                    bridge: bridge.clone_ref(py),
                })
                .map_err(|error| report(py, &error))
        })
    }

    /// Executes a `*_dictionary.py` like the original `_load_translation_file`
    /// (in an isolated module namespace) and returns its `translations` dict.
    pub fn dictionary_evaluator(&self) -> Box<Evaluator> {
        let bridge = Python::attach(|py| self.bridge.clone_ref(py));
        Box::new(move |path: &Path| {
            Python::attach(|py| {
                bridge
                    .bind(py)
                    .call_method1("load_translation_file", (path.as_os_str(),))
                    .and_then(|pairs| pairs.extract::<Dictionary>())
                    .map_err(|error| format!("{}: {error}", path.display()))
            })
        })
    }

    /// Metadata of `path` without importing it; `None` when no top-level class
    /// assigns a usable `widgetname` (the original then imports the file
    /// synchronously at startup).
    pub fn scan(&self, path: &Path) -> Option<ScannedWidget> {
        Python::attach(|py| {
            let scanned = self
                .bridge
                .bind(py)
                .call_method1("scan_widget_metadata_ex", (path.as_os_str(),))
                .and_then(|metadata| {
                    metadata.extract::<Option<(String, Option<String>, String, String)>>()
                });
            match scanned {
                Ok(metadata) => {
                    metadata.map(
                        |(widget_name, icon_path, widget_id, class_name)| ScannedWidget {
                            widget_name,
                            icon_path,
                            widget_id,
                            class_name,
                        },
                    )
                }
                Err(error) => {
                    report(py, &error);
                    None
                }
            }
        })
    }

    /// Import (execute) `path` on the calling thread, as a fresh module named
    /// after the file stem (original `spec_from_file_location(module_name, filepath)`).
    pub fn import_module(&self, path: &Path) -> Result<Module, String> {
        import(&self.bridge, path)
    }

    /// Import `path` on a worker thread (original `_start_bg_import`) and call
    /// `done` on the GTK main thread with the result.
    pub fn import_in_background(
        &self,
        path: PathBuf,
        done: impl FnOnce(Result<Module, String>) + 'static,
    ) {
        let bridge = Python::attach(|py| self.bridge.clone_ref(py));
        let done: ImportDone = Box::new(done);
        let pending = Arc::new(Mutex::new(Some(ThreadGuard::new(done))));
        let worker_pending = Arc::clone(&pending);
        let worker_path = path.clone();
        let spawned = thread::Builder::new()
            .name("linexin-widget-import".into())
            .spawn(move || {
                let result = import(&bridge, &worker_path);
                glib::idle_add_once(move || finish_import(&worker_pending, result));
            });
        if let Err(error) = spawned {
            let message = format!(
                "{}: cannot start the import thread: {error}",
                path.display()
            );
            finish_import(&pending, Err(message));
        }
    }

    /// Original `_prepare_module` + `_extract_widgets_from_module`: inject
    /// `_`, `sudo_manager`, `translate_dialog`, `translate_window` (dictionary
    /// domain chosen with `Localization::domain_for_module` of the file stem),
    /// then construct every public `Gtk.Widget` subclass defined in the module
    /// that declares a `widgetname`, passing `window` plus the accepted subset of
    /// [`WidgetArgs`]. GTK main thread only. One entry per class tried.
    pub fn instantiate(
        &self,
        module: &Module,
        window: &gtk::Window,
        args: &WidgetArgs,
    ) -> Vec<Result<LoadedWidget, String>> {
        let stem = module
            .path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let domain = linexin_localization::global().domain_for_module(&stem);
        // The original created the shared SudoManager while preparing a module.
        linexin_services::sudo::manager();
        Python::attach(|py| {
            let extracted = || -> PyResult<Vec<Result<LoadedWidget, String>>> {
                let bridge = self.bridge.bind(py);
                let path = module.path.as_os_str();
                let python_module = module.module.bind(py);
                bridge.call_method1("prepare_module", (python_module, path, &domain))?;
                let window = bridge.call_method1("window_wrapper", (address(window),))?;
                bridge
                    .call_method1(
                        "extract_widgets",
                        (
                            python_module,
                            path,
                            window,
                            args.hide_sidebar,
                            args.voice_autostart,
                            args.conversation_id.as_deref(),
                        ),
                    )?
                    .try_iter()?
                    .map(|entry| loaded_widget(&entry?))
                    .collect()
            };
            extracted().unwrap_or_else(|error| vec![Err(report(py, &error))])
        })
    }

    /// Legacy `get_header_bar_widget()` (single-widget mode), if defined and
    /// returning a widget.
    pub fn header_bar_widget(&self, widget: &LoadedWidget) -> Option<gtk::Widget> {
        Python::attach(|py| {
            let header = || -> PyResult<Option<gtk::Widget>> {
                let bridge = self.bridge.bind(py);
                let instance = bridge.call_method1("wrap", (address(&widget.widget),))?;
                let header = bridge.call_method1("header_bar_widget", (instance,))?;
                if header.is_none() {
                    return Ok(None);
                }
                widget_from_python(&header)
            };
            header().unwrap_or_else(|error| {
                report(py, &error);
                None
            })
        })
    }

    /// Expose host attributes on the Python wrapper of `window`, as the original
    /// `WidgetLoaderWindow` had them: `hide_sidebar`, `active_widget_id`,
    /// `_compact_mode`, `refresh_widgets()`, `resize_window(w, h)`, `resize_and_center(w, h)`,
    /// `is_command_locked`. GTK main thread only.
    pub fn install_window(&self, window: &gtk::Window, hide_sidebar: bool, hooks: WindowHooks) {
        let key = address(window);
        HOOKS.with(|installed| installed.borrow_mut().insert(key, Rc::new(hooks)));
        self.call(|bridge| bridge.call_method1("install_window", (key, hide_sidebar)));
    }

    /// Update `window.active_widget_id` (used by `translate_dialog` without an
    /// explicit domain).
    pub fn set_active_widget_id(&self, window: &gtk::Window, widget_id: Option<&str>) {
        Python::attach(|py| {
            let set = || -> PyResult<()> {
                self.bridge
                    .bind(py)
                    .call_method1("window_wrapper", (address(window),))?
                    .setattr("active_widget_id", widget_id)
            };
            if let Err(error) = set() {
                report(py, &error);
            }
        });
    }

    /// Update `window._compact_mode` (the original window's flag, which
    /// Alexy AI reads to place its conversations list).
    pub fn set_compact_mode(&self, window: &gtk::Window, compact: bool) {
        Python::attach(|py| {
            let set = || -> PyResult<()> {
                self.bridge
                    .bind(py)
                    .call_method1("window_wrapper", (address(window),))?
                    .setattr("_compact_mode", compact)
            };
            if let Err(error) = set() {
                report(py, &error);
            }
        });
    }

    /// Original `do_close_request` hook: call `_cleanup_screenshot_tmp()` on
    /// every loaded widget that has it.
    pub fn close_request(&self) {
        self.call(|bridge| bridge.call_method0("close_request"));
    }

    /// Original SIGUSR1 handler for a widget with `stt_toggle` (re-toggle it).
    pub fn reactivate_voice(&self, widget: &LoadedWidget) {
        self.call(|bridge| {
            let instance = bridge.call_method1("wrap", (address(&widget.widget),))?;
            bridge.call_method1("reactivate_voice", (instance,))
        });
    }

    /// Original `_launch_compact_voice`: import `path` with injected helpers
    /// and construct its `CompactVoiceWindow(application=…, voice_autostart=…)`.
    pub fn launch_compact_voice(
        &self,
        application: &adw::Application,
        path: &Path,
        voice_autostart: bool,
    ) -> Result<gtk::Window, String> {
        linexin_services::sudo::manager();
        Python::attach(|py| {
            let launch = || -> PyResult<gtk::Window> {
                let bridge = self.bridge.bind(py);
                let application = bridge.call_method1("wrap", (address(application),))?;
                let window = bridge.call_method1(
                    "launch_compact_voice",
                    (application, path.as_os_str(), voice_autostart),
                )?;
                object_from_python(&window)?
                    .and_then(|window| window.downcast::<gtk::Window>().ok())
                    .ok_or_else(|| PyTypeError::new_err("CompactVoiceWindow is not a Gtk.Window"))
            };
            launch().map_err(|error| report(py, &error))
        })
    }

    /// SIGUSR1 handler for a compact voice window (`_mic_btn` re-toggle).
    pub fn reactivate_compact_voice(&self, window: &gtk::Window) {
        self.call(|bridge| {
            let window = bridge.call_method1("wrap", (address(window),))?;
            bridge.call_method1("reactivate_compact_voice", (window,))
        });
    }

    /// Set `sys.argv` (the original script saw `[script path, *unparsed args]`)
    /// and put the executable's directory first on `sys.path`, as running the
    /// script did. Call before importing any widget.
    pub fn set_argv(&self, argv: &[String]) {
        Python::attach(|py| {
            let result = py.import("sys").and_then(|sys| {
                sys.setattr("argv", argv.to_vec())?;
                if let Some(directory) = argv
                    .first()
                    .and_then(|executable| Path::new(executable).parent())
                {
                    sys.getattr("path")?
                        .call_method1("insert", (0, directory.as_os_str()))?;
                }
                Ok(())
            });
            if let Err(error) = result {
                error.print(py);
            }
        });
    }

    /// Release host resources after the application ends. Idempotent.
    pub fn shutdown(&self) {
        // The interpreter is never finalized (live GTK wrappers and threads),
        // so do the user-visible part of Py_FinalizeEx the original's exit
        // did: wait for non-daemon threads, run atexit handlers (widgets
        // register their own cleanups there), then flush what widgets printed
        // (block-buffered when stdout is not a terminal).
        Python::attach(|py| {
            let finalize =
                c"import threading, atexit\nthreading._shutdown()\natexit._run_exitfuncs()";
            if let Err(error) = py.run(finalize, None, None) {
                error.print(py);
            }
        });
        // The original registered its SudoManager cleanup with atexit first,
        // so it ran after the widgets' own handlers.
        if let Some(manager) = linexin_services::sudo::existing() {
            manager.cleanup();
        }
        Python::attach(|py| {
            if let Ok(sys) = py.import("sys") {
                for stream in ["stdout", "stderr"] {
                    if let Ok(stream) = sys.getattr(stream) {
                        let _ = stream.call_method0("flush");
                    }
                }
            }
        });
    }

    /// Run bridge calls whose result is irrelevant; errors are printed.
    fn call(
        &self,
        body: impl for<'py> FnOnce(&Bound<'py, PyModule>) -> PyResult<Bound<'py, PyAny>>,
    ) {
        Python::attach(|py| {
            if let Err(error) = body(self.bridge.bind(py)) {
                report(py, &error);
            }
        });
    }
}

fn load_bridge(py: Python<'_>) -> PyResult<Py<PyModule>> {
    py.import("sys")?.setattr("dont_write_bytecode", true)?;
    host::register(py)?;
    let bridge = PyModule::from_code(py, BRIDGE_SOURCE, c"linexin_bridge.py", c"_linexin_bridge")?;
    Ok(bridge.unbind())
}

fn import(bridge: &Py<PyModule>, path: &Path) -> Result<Module, String> {
    Python::attach(|py| {
        bridge
            .bind(py)
            .call_method1("import_module", (path.as_os_str(),))
            .map(|module| Module {
                path: path.to_owned(),
                module: module.unbind(),
            })
            .map_err(|error| report(py, &error))
    })
}

fn finish_import(pending: &Mutex<Option<ThreadGuard<ImportDone>>>, result: Result<Module, String>) {
    let done = pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(done) = done {
        (done.into_inner())(result);
    }
}

fn loaded_widget(entry: &Bound<'_, PyAny>) -> PyResult<Result<LoadedWidget, String>> {
    let (class_name, instance, details): (String, Bound<'_, PyAny>, Bound<'_, PyAny>) =
        entry.extract()?;
    if instance.is_none() {
        return Ok(Err(details.extract()?));
    }
    let (widget_name, icon_path, widget_id) = details.extract()?;
    let Some(widget) = widget_from_python(&instance)? else {
        return Ok(Err(format!("{class_name} is not a GTK widget")));
    };
    Ok(Ok(LoadedWidget {
        widget,
        widget_name,
        icon_path,
        widget_id,
        class_name,
    }))
}

fn address(object: &impl IsA<glib::Object>) -> usize {
    object.upcast_ref::<glib::Object>().as_ptr() as usize
}

/// The GObject behind a PyGObject wrapper; `None` for other Python objects.
fn object_from_python(object: &Bound<'_, PyAny>) -> PyResult<Option<glib::Object>> {
    let gobject = object
        .py()
        .import("gi.repository.GObject")?
        .getattr("Object")?;
    if !object.is_instance(&gobject)? {
        return Ok(None);
    }
    let Ok(capsule) = object.getattr("__gpointer__") else {
        return Ok(None);
    };
    let Ok(capsule) = capsule.cast_into::<PyCapsule>() else {
        return Ok(None);
    };
    let pointer = capsule.pointer_checked(None)?;
    // SAFETY: PyGObject's `__gpointer__` holds the wrapped, live `GObject*`
    // (kept alive by `object`); `from_glib_none` takes a new reference.
    let object: glib::Object =
        unsafe { from_glib_none(pointer.as_ptr().cast::<glib::gobject_ffi::GObject>()) };
    Ok(Some(object))
}

fn widget_from_python(object: &Bound<'_, PyAny>) -> PyResult<Option<gtk::Widget>> {
    Ok(object_from_python(object)?.and_then(|object| object.downcast().ok()))
}

fn on_main_thread() -> bool {
    MAIN_THREAD.get() == Some(&thread::current().id())
}

fn refresh_widgets(window: usize) {
    let hooks = HOOKS.with(|installed| installed.borrow().get(&window).cloned());
    if let Some(hooks) = hooks {
        (hooks.refresh_widgets)();
    }
}

/// Print a Python exception like `traceback.print_exc()` and return the text.
fn report(py: Python<'_>, error: &PyErr) -> String {
    let text = py
        .import("traceback")
        .and_then(|traceback| traceback.call_method1("format_exception", (error.value(py),)))
        .and_then(|lines| lines.extract::<Vec<String>>())
        .map(|lines| lines.concat())
        .unwrap_or_else(|_| format!("{error}\n"));
    eprint!("{text}");
    text
}
