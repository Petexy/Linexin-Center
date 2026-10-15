//! Interpreter-level tests that need no display: metadata scan, dictionary
//! evaluation, imports and the sudo_manager facade over the real Rust services
//! (driven by a fake `sudo` first in `PATH`). Widget construction and
//! rendering need a compositor and are covered by the `bridge_check` example.

use std::cell::{Cell, RefCell};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use gtk::glib;
use linexin_python_bridge::{PythonRuntime, ScannedWidget};
use pyo3::prelude::*;

const FAKE_SUDO: &str = r#"#!/bin/sh
case "$1" in
    -k) exit 0 ;;
    -S) IFS= read -r password || exit 1
        [ "$password" = "correct horse" ] && exit 0
        exit 1 ;;
    -A) shift
        password=$("$SUDO_ASKPASS") || exit 1
        [ "$password" = "correct horse" ] || exit 1
        exec "$@" ;;
esac
exit 2
"#;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/python/fixtures/bridge")
}

const INSTALLED: &str = "/usr/share/linexin/widgets";

/// The runtime, with a fake `sudo` first in `PATH` and temporary files in a
/// private directory (set once, before any test spawns processes).
fn runtime() -> PythonRuntime {
    test_dir();
    PythonRuntime::new().expect("Python runtime")
}

fn test_dir() -> &'static Path {
    static ENVIRONMENT: OnceLock<PathBuf> = OnceLock::new();
    ENVIRONMENT.get_or_init(|| {
        let dir = std::env::temp_dir().join("linexin-python-bridge-tests");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("tmp")).unwrap();
        fs::write(dir.join("sudo"), FAKE_SUDO).unwrap();
        fs::set_permissions(dir.join("sudo"), fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::var_os("PATH").unwrap_or_default();
        let paths = std::iter::once(dir.clone()).chain(std::env::split_paths(&path));
        let path = std::env::join_paths(paths).unwrap();
        // SAFETY: every test reaches this initializer first; the others block
        // on the OnceLock meanwhile.
        unsafe {
            std::env::set_var("PATH", path);
            std::env::set_var("TMPDIR", dir.join("tmp"));
        }
        dir
    })
}

/// Serializes the tests that iterate the default main context.
fn main_context_user() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

fn iterate_until(what: &str, condition: impl Fn() -> bool) {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        context.iteration(false);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn runtime_is_shared_and_never_writes_bytecode() {
    let first = runtime();
    let _second = runtime();
    Python::attach(|py| {
        let sys = py.import("sys").unwrap();
        assert!(
            sys.getattr("dont_write_bytecode")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        let modules = sys.getattr("modules").unwrap();
        assert!(modules.contains("_linexin_host").unwrap());
        assert!(modules.contains("_linexin_bridge").unwrap());
    });
    let dir = test_dir().join("bytecode");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("helper_module.py"), "VALUE = 1\n").unwrap();
    let path = dir.join("z-plain-widget.py");
    fs::write(
        &path,
        "import sys\nsys.path.insert(0, __file__.rsplit('/', 1)[0])\nimport helper_module\n",
    )
    .unwrap();
    assert_eq!(first.import_module(&path).unwrap().path(), path);
    assert!(!dir.join("__pycache__").exists());
}

#[test]
fn scan_reads_metadata_without_importing() {
    let runtime = runtime();
    assert_eq!(
        runtime.scan(&fixtures().join("a-gettext_widget.py")),
        Some(ScannedWidget {
            widget_name: "Fixture Gettext".into(),
            icon_path: Some("/usr/share/icons/fixture-gettext.svg".into()),
            widget_id: "fixture_gettext".into(),
            class_name: "GettextWidget".into(),
        })
    );
    // Importing this one would fail; scanning only parses it.
    assert_eq!(
        runtime
            .scan(&fixtures().join("g-import_error.py"))
            .map(|widget| widget.widget_name),
        Some("Fixture Import Error".into())
    );
    assert_eq!(
        runtime.scan(&fixtures().join("h-dynamic_name_widget.py")),
        None
    );
    assert_eq!(runtime.scan(Path::new("/nonexistent/widget.py")), None);
}

#[test]
fn scan_installed_widgets() {
    let runtime = runtime();
    let Ok(entries) = fs::read_dir(INSTALLED) else {
        return;
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "py") {
            let scanned = runtime.scan(&path);
            assert!(scanned.is_some(), "{} has no metadata", path.display());
            found.extend(scanned.map(|widget| (widget.widget_name, widget.widget_id)));
        }
    }
    if Path::new(INSTALLED)
        .join("z-system-information-widget.py")
        .exists()
    {
        assert!(found.contains(&("System Information".into(), "system_information".into())));
    }
}

#[test]
fn dictionary_evaluator_matches_the_original_loader() {
    let evaluate = runtime().dictionary_evaluator();
    let dictionary =
        Path::new(INSTALLED).join("localization/pl_PL/system_information_dictionary.py");
    if dictionary.exists() {
        let pairs = evaluate(&dictionary).unwrap();
        assert_eq!(
            pairs.first(),
            Some(&("System Information".into(), "Informacje o Systemie".into()))
        );
    }
    // Alexy AI's dictionary needs execution (variables, translations.update).
    let alexy = Path::new(INSTALLED).join("localization/pl_PL/aa_alexy_ai_dictionary.py");
    if alexy.exists() {
        let pairs = evaluate(&alexy).unwrap();
        assert!(pairs.iter().any(|(key, _)| key == "Alexy AI"));
        assert!(pairs.len() > 100);
    }
    let error = evaluate(&fixtures().join("g-import_error.py")).unwrap_err();
    assert!(error.contains("ZeroDivisionError"), "{error}");
}

#[test]
fn import_errors_carry_the_traceback() {
    let runtime = runtime();
    let error = match runtime.import_module(&fixtures().join("g-import_error.py")) {
        Ok(_) => panic!("the import should fail"),
        Err(error) => error,
    };
    assert!(error.contains("Traceback"), "{error}");
    assert!(
        error.contains("ZeroDivisionError: fixture import failure"),
        "{error}"
    );
    assert!(
        runtime
            .import_module(Path::new("/nonexistent/widget.py"))
            .is_err()
    );
}

#[test]
fn background_import_keeps_the_main_loop_running() {
    let _context = main_context_user();
    let runtime = runtime();
    let path = test_dir().join("slow-widget.py");
    fs::write(&path, "import time\ntime.sleep(0.8)\nVALUE = __name__\n").unwrap();

    let ticks = Rc::new(Cell::new(0u32));
    let counter = Rc::clone(&ticks);
    let timer = glib::timeout_add_local(Duration::from_millis(20), move || {
        counter.set(counter.get() + 1);
        glib::ControlFlow::Continue
    });
    let result = Rc::new(RefCell::new(None));
    let slot = Rc::clone(&result);
    let main_thread = std::thread::current().id();
    runtime.import_in_background(path.clone(), move |module| {
        assert_eq!(std::thread::current().id(), main_thread);
        *slot.borrow_mut() = Some(module);
    });
    iterate_until("the background import", || result.borrow().is_some());
    timer.remove();
    let module = result.borrow_mut().take().unwrap();
    assert_eq!(module.unwrap().path(), path);
    assert!(
        ticks.get() >= 20,
        "main loop starved: {} ticks",
        ticks.get()
    );
}

#[test]
fn sudo_manager_facade_over_the_rust_services() {
    let _context = main_context_user();
    runtime();
    let applied = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&applied);
    let context = glib::MainContext::default();
    while context.pending() {
        context.iteration(false);
    }
    linexin_services::lock::set_apply_callback(move |locked| record.borrow_mut().push(locked));

    let script = c"
import subprocess, _linexin_bridge
sm = _linexin_bridge.sudo_manager
assert not _linexin_bridge._should_lock_command((['true'],), {})
wrapper = sm.wrapper_path
assert isinstance(wrapper, str) and wrapper == sm.wrapper_path
assert _linexin_bridge._should_lock_command(([wrapper, 'true'],), {})
assert sm.fifo_path.startswith(sm.fifo_dir + '/')
assert not sm.validate_password('wrong')
assert sm.validate_password('correct horse')
assert sm.user_password is None
try:
    sm.run_privileged(['true'])
    raise AssertionError('ran without a password')
except ValueError as error:
    assert str(error) == 'No password set'
sm.set_password('correct horse')
assert sm.user_password == 'correct horse'
assert sm.run_privileged(['sh', '-c', 'exit 4']).returncode == 4
sm.start_privileged_session()
process = subprocess.Popen([sm.wrapper_path, 'true'], env=sm.get_env())
first = process.wait()
second = subprocess.run([sm.wrapper_path, 'sh', '-c', 'exit 5']).returncode
sm.stop_privileged_session()
sm.forget_password()
assert (first, second, sm.user_password) == (0, 5, None)
";
    Python::attach(|py| py.run(script, None, None)).unwrap_or_else(|error| {
        Python::attach(|py| panic!("{}", error.value(py)));
    });
    iterate_until("the UI unlock", || {
        !linexin_services::lock::is_locked() && applied.borrow().last() == Some(&false)
    });
    let applied = applied.borrow();
    assert!(applied.len() >= 2 && applied.len() % 2 == 0, "{applied:?}");
    assert!(
        applied.chunks(2).all(|pair| pair == [true, false]),
        "{applied:?}"
    );

    let wrapper = linexin_services::sudo::manager().wrapper_path().to_owned();
    assert!(wrapper.exists());
    runtime().shutdown();
    runtime().shutdown();
    assert!(!wrapper.exists());
}
