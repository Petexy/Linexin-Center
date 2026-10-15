//! Port of the original `WidgetLoaderApp` and `main`.

use crate::catalog::{self, Kind};
use crate::config::{self, ConfigFile};
use crate::native::NativeWidget;
use crate::options::{self, Options, Parsed};
use crate::paths::{self, Paths};
use crate::window::{CenterWindow, Services, WindowOptions};
use adw::prelude::*;
use gtk::{gio, glib};
use linexin_localization::{Localization, detect_language, global};
use linexin_python_bridge::PythonRuntime;
use std::cell::{Cell, RefCell};
use std::process::ExitCode;
use std::rc::{Rc, Weak};

/// The window SIGUSR1 talks to (original `WidgetLoaderApp.window`).
enum Current {
    None,
    Center(CenterWindow),
    CompactVoice(glib::WeakRef<gtk::Window>),
    /// A native widget's `--compact` window.
    NativeCompactVoice(glib::WeakRef<gtk::Window>, Rc<NativeWidget>),
}

struct App {
    options: Options,
    services: Rc<Services>,
    windows: RefCell<Vec<CenterWindow>>,
    current: RefCell<Current>,
    signal_installed: Cell<bool>,
}

pub fn run() -> ExitCode {
    let options = match options::parse(std::env::args().skip(1)) {
        Parsed::Run(options) => options,
        Parsed::Print(text) => {
            println!("{text}");
            return ExitCode::SUCCESS;
        }
        Parsed::Fail(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    for arg in &options.ignored {
        eprintln!("Ignoring unknown argument: {arg}");
    }
    linexin_python_bridge::preinitialize();
    if options.list {
        return list_widgets(&options);
    }
    if crate::lxb_front::wanted() {
        match crate::lxb_front::run(&options) {
            Ok(code) => return code,
            Err(why) => eprintln!("{why}; using the GTK window instead"),
        }
    }
    let kinexin = paths::detect_plasma_or_hyprland();
    if let Err(error) = adw::init() {
        eprintln!("Cannot initialize GTK/libadwaita: {error}");
        return ExitCode::FAILURE;
    }
    let python = match PythonRuntime::new() {
        Ok(runtime) => {
            runtime.set_argv(&python_argv(&options));
            Some(Rc::new(runtime))
        }
        Err(error) => {
            eprintln!("Python widgets are unavailable: {error}");
            None
        }
    };
    let paths = Paths::resolve(&options.widgets_dirs);
    let language = detect_language(options.language.as_deref());
    let evaluator = python.as_ref().map(|python| python.dictionary_evaluator());
    linexin_localization::install(Localization::load(
        &paths.localization_dirs(),
        &language,
        evaluator.as_deref(),
    ));
    println!("System language detected: {}", global().language());
    for diagnostic in global().diagnostics() {
        eprintln!("{diagnostic}");
    }

    let new_instance = std::env::var_os("LINEXIN_NEW_INSTANCE").is_some_and(|v| !v.is_empty());
    let flags = if new_instance {
        gio::ApplicationFlags::NON_UNIQUE
    } else {
        gio::ApplicationFlags::empty()
    };
    let application = adw::Application::new(Some(paths::APP_ID), flags);
    let app = Rc::new(App {
        options,
        services: Rc::new(Services {
            paths,
            config: ConfigFile::from_env(),
            python,
            kinexin,
        }),
        windows: RefCell::default(),
        current: RefCell::new(Current::None),
        signal_installed: Cell::new(false),
    });
    let weak = Rc::downgrade(&app);
    application.connect_activate(move |application| {
        if let Some(app) = weak.upgrade() {
            app.on_activate(application);
        }
    });
    // The original's PyGObject Application.run quit on Ctrl+C. Embedded
    // Python would otherwise own SIGINT and raise KeyboardInterrupt inside
    // whichever widget code runs next instead of quitting.
    glib::unix_signal_add_local(libc::SIGINT, {
        let application = application.downgrade();
        move || {
            if let Some(application) = application.upgrade() {
                application.quit();
            }
            glib::ControlFlow::Continue
        }
    });
    let program = std::env::args()
        .next()
        .unwrap_or_else(|| "linexin-center".into());
    let status = application.run_with_args(&[program]);
    // The original's cleanup ran when its interpreter finalized after
    // app.run() returned: waiting for widget threads, then atexit handlers.
    if let Some(python) = &app.services.python {
        python.shutdown();
    }
    if let Some(manager) = linexin_services::sudo::existing() {
        manager.cleanup();
    }
    drop(app);
    ExitCode::from(u8::from(status))
}

impl App {
    /// Original `on_activate`: every activation opens a window.
    fn on_activate(self: &Rc<Self>, application: &adw::Application) {
        let options = &self.options;
        if options.compact_mode && options.target_widget.is_some() {
            self.launch_compact_voice(application);
            return;
        }
        let window = CenterWindow::new(
            application,
            self.services.clone(),
            WindowOptions {
                target_widget: options.target_widget.clone(),
                voice_autostart: options.voice_autostart,
                conversation_id: options.conversation_id.clone(),
            },
        );
        let Some(gtk_window) = window.window() else {
            return;
        };
        let weak = Rc::downgrade(self);
        gtk_window.connect_destroy(move |destroyed| {
            if let Some(app) = weak.upgrade() {
                app.windows
                    .borrow_mut()
                    .retain(|w| w.window().is_some_and(|w| &w != destroyed));
            }
        });
        self.windows.borrow_mut().push(window.clone());
        self.current.replace(Current::Center(window));
        gtk_window.present();
        self.install_sigusr1();
    }

    /// Original `_launch_compact_voice`: the widget's floating voice window,
    /// a native widget's own when it has one.
    fn launch_compact_voice(self: &Rc<Self>, application: &adw::Application) {
        let target = self.options.target_widget.as_deref().unwrap_or_default();
        let dirs = &self.services.paths.widgets_dirs;
        let native = dirs
            .iter()
            .map(|dir| dir.join(format!("{target}.so")))
            .find(|path| path.is_file());
        if let Some(path) = native {
            match NativeWidget::load(&path) {
                Ok(native) if native.has_compact_voice() => {
                    let native = Rc::new(native);
                    match native.create_compact_voice(
                        application.upcast_ref(),
                        self.options.voice_autostart,
                    ) {
                        Ok(window) => {
                            self.current.replace(Current::NativeCompactVoice(
                                window.downgrade(),
                                native.clone(),
                            ));
                            window.present();
                            self.install_sigusr1();
                        }
                        Err(error) => println!("Error: {error}"),
                    }
                    return;
                }
                Ok(_) => {}
                Err(error) => {
                    println!("Error: cannot load widget {}: {error}", path.display());
                    return;
                }
            }
        }
        let file_name = format!("{target}.py");
        let Some(path) = dirs
            .iter()
            .map(|dir| dir.join(&file_name))
            .find(|path| path.is_file())
        else {
            let expected = dirs
                .first()
                .map(|dir| dir.join(&file_name))
                .unwrap_or_default();
            println!("Error: widget file not found: {}", expected.display());
            return;
        };
        let Some(python) = &self.services.python else {
            println!("Error: cannot load widget module: {}", path.display());
            return;
        };
        match python.launch_compact_voice(application, &path, self.options.voice_autostart) {
            Ok(window) => {
                self.current
                    .replace(Current::CompactVoice(window.downgrade()));
                window.present();
                self.install_sigusr1();
            }
            Err(error) => println!("Error: {error}"),
        }
    }

    /// SIGUSR1 restarts voice input of the active widget (original handlers).
    fn install_sigusr1(self: &Rc<Self>) {
        if self.signal_installed.replace(true) {
            return;
        }
        let weak: Weak<Self> = Rc::downgrade(self);
        glib::unix_signal_add_local(libc::SIGUSR1, move || {
            if let Some(app) = weak.upgrade() {
                app.reactivate_voice();
            }
            glib::ControlFlow::Continue
        });
    }

    fn reactivate_voice(&self) {
        match &*self.current.borrow() {
            Current::Center(window) => window.reactivate_voice(),
            Current::CompactVoice(window) => {
                if let (Some(window), Some(python)) = (window.upgrade(), &self.services.python) {
                    python.reactivate_compact_voice(&window);
                }
            }
            Current::NativeCompactVoice(window, native) => {
                if let Some(window) = window.upgrade() {
                    native.reactivate_compact_voice(&window);
                }
            }
            Current::None => {}
        }
    }
}

/// The `sys.argv` the original script saw once `_parse_arguments` removed
/// its own options: the executable path, then the arguments it did not know.
fn python_argv(options: &Options) -> Vec<String> {
    let executable = std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "linexin-center".into());
    std::iter::once(executable)
        .chain(options.ignored.iter().cloned())
        .collect()
}

/// `--list`: the widget files in sidebar order, without GTK. Native
/// descriptors are loaded and validated; problems go to stderr.
fn list_widgets(options: &Options) -> ExitCode {
    let paths = Paths::resolve(&options.widgets_dirs);
    let saved = config::names(&ConfigFile::from_env().load(), config::WIDGET_ORDER);
    let mut failed = false;
    for file in catalog::sidebar_order(&paths.widgets_dirs, saved) {
        match file.kind {
            Kind::Native => match NativeWidget::load(&file.path) {
                Ok(native) => println!(
                    "{}\tnative\t{}\t{}\t{}",
                    file.file_name,
                    native.widget_id,
                    native.name,
                    file.path.display()
                ),
                Err(error) => {
                    eprintln!("{}: {error}", file.path.display());
                    failed = true;
                }
            },
            Kind::Python => println!("{}\tpython\t\t\t{}", file.file_name, file.path.display()),
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
