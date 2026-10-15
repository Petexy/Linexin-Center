//! Without a usable lxb-toolkit the Center falls back to its GTK window. The
//! real binary runs with `LINEXIN_FRONTEND=lxb` against a `liblxb_app.so`
//! found first through `LD_LIBRARY_PATH`: an empty file, or the fake of
//! `tests/fixtures/lxb/fake_lxb_app.rs` failing each way a real one can.
//! There is no display (`WAYLAND_DISPLAY` names a socket that does not
//! exist, `DISPLAY` is unset), so the GTK window cannot open either: what is
//! checked is what the Center says on its way there. Everything it writes
//! stays in cargo's temporary folder for tests.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const FALLBACK: &str = "; using the GTK window instead";
const NO_GTK: &str = "Cannot initialize GTK/libadwaita";
const LANGUAGE: &str = "System language detected";

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// A scratch directory of the test's own, emptied first.
fn scratch(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("lxb-fallback-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    for dir in ["lib", "widgets", "home", "config", "runtime", "tmp"] {
        std::fs::create_dir_all(root.join(dir)).expect("test directory");
    }
    root
}

/// Build the fake into `root/lib/liblxb_app.so` (`old`: without `lxb_app_run`).
fn build_fake(root: &Path, old: bool) {
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/lxb/fake_lxb_app.rs");
    let mut rustc = Command::new("rustc");
    rustc.args(["--edition=2024", "--crate-type=cdylib"]);
    if old {
        rustc.args(["--cfg", "old"]);
    }
    let status = rustc
        .arg(&source)
        .arg("-o")
        .arg(root.join("lib/liblxb_app.so"))
        .status()
        .expect("rustc");
    assert!(status.success(), "building {}", source.display());
}

/// The Center on the LineXinBar front end with `root/lib` first on the
/// library path, `FAKE_LXB_APP=mode`, and nothing of the real session.
fn run_center(root: &Path, mode: &str) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_linexin-center"))
        .arg("--widgets-dir")
        .arg(root.join("widgets"))
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("TMPDIR", root.join("tmp"))
        .env("LANG", "en_US.UTF-8")
        .env("WAYLAND_DISPLAY", "no-such-compositor")
        .env("GDK_BACKEND", "wayland")
        .env("LD_LIBRARY_PATH", root.join("lib"))
        .env("LINEXIN_FRONTEND", "lxb")
        .env("LINEXIN_NEW_INSTANCE", "1")
        .env("FAKE_LXB_APP", mode)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("linexin-center");
    let started = Instant::now();
    while child.try_wait().expect("wait").is_none() {
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            panic!("linexin-center ({mode}) did not exit within a minute");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child.wait_with_output().expect("output");
    Run {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// The Center gave up on LineXinBar saying `why`, before setting anything up
/// for it, and went on to the GTK window (which has no display here).
fn assert_fell_back(run: &Run, why: &str) {
    assert!(
        run.stderr.contains(&format!("{why}{FALLBACK}")),
        "expected \"{why}{FALLBACK}\" in:\n{}",
        run.stderr
    );
    assert!(run.stderr.contains(NO_GTK), "{}", run.stderr);
    assert!(!run.stdout.contains(LANGUAGE), "{}", run.stdout);
    assert_eq!(run.code, Some(1));
}

#[test]
fn a_library_that_cannot_be_opened_means_the_gtk_window() {
    let root = scratch("unopenable");
    std::fs::write(root.join("lib/liblxb_app.so"), b"").expect("empty library");
    let run = run_center(&root, "");
    assert_fell_back(&run, "file too short");
    assert!(
        run.stderr
            .contains("LineXinBar's liblxb_app.so is not available")
    );
}

#[test]
fn a_toolkit_too_old_means_the_gtk_window() {
    let root = scratch("old");
    build_fake(&root, true);
    let run = run_center(&root, "no-window");
    assert_fell_back(&run, "liblxb_app.so lacks lxb_app_run");
}

#[test]
fn an_application_that_cannot_be_made_means_the_gtk_window() {
    let root = scratch("null");
    build_fake(&root, false);
    let run = run_center(&root, "null");
    assert_fell_back(&run, "lxb_app_new answered null");
}

#[test]
fn a_window_that_cannot_open_means_the_gtk_window() {
    let root = scratch("no-window");
    build_fake(&root, false);
    let run = run_center(&root, "no-window");
    assert_fell_back(
        &run,
        "LineXinBar's window could not open: no window: no display (fake)",
    );
}

#[test]
fn a_window_lost_after_it_opened_ends_the_center() {
    let root = scratch("after-frame");
    build_fake(&root, false);
    let run = run_center(&root, "after-frame");
    // The first frame set the front end up; the GTK window does not follow.
    assert_eq!(run.stdout.matches(LANGUAGE).count(), 1, "{}", run.stdout);
    assert!(
        run.stderr
            .contains("Linexin Center: the display went away (fake)"),
        "{}",
        run.stderr
    );
    assert!(!run.stderr.contains(FALLBACK), "{}", run.stderr);
    assert!(!run.stderr.contains(NO_GTK), "{}", run.stderr);
    assert_eq!(run.code, Some(1));
}
