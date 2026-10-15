//! Tests run against a fake `sudo` placed first in `PATH`: it never elevates
//! anything, it only checks the password supplied through `$SUDO_ASKPASS`.

use super::*;
use crate::lock;
use std::cell::RefCell;
use std::os::unix::fs::FileTypeExt;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Instant;

use gtk::glib;

const PASSWORD: &str = "correct horse";

const FAKE_SUDO: &str = r#"#!/bin/sh
log="$(dirname "$0")/sudo.log"
case "$1" in
    -k) echo k >> "$log"; exit 0 ;;
    -S) IFS= read -r password || exit 1
        [ "$password" = "correct horse" ] && exit 0
        exit 1 ;;
    -A) shift
        # Simulates sudo's cached credentials: askpass is never run.
        if [ "$1" = "--cached" ]; then shift; exec "$@"; fi
        password=$("$SUDO_ASKPASS") || exit 1
        [ "$password" = "correct horse" ] || exit 1
        exec "$@" ;;
esac
exit 2
"#;

/// Directory holding the fake `sudo` (and its log), put first in `PATH` once;
/// the managers' temporary files go to its `tmp` subdirectory.
fn fake_sudo_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join("linexin-services-tests");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("tmp")).expect("create fake sudo dir");
        let sudo = dir.join("sudo");
        fs::write(&sudo, FAKE_SUDO).expect("write fake sudo");
        fs::set_permissions(&sudo, fs::Permissions::from_mode(0o755)).expect("chmod fake sudo");
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![dir.clone()];
        paths.extend(std::env::split_paths(&path));
        let joined = std::env::join_paths(paths).expect("join PATH");
        // SAFETY: every test reaches this initializer before spawning
        // processes or threads; others block on the OnceLock meanwhile.
        unsafe {
            std::env::set_var("PATH", joined);
            std::env::set_var("TMPDIR", dir.join("tmp"));
        }
        dir
    })
}

/// Serializes the tests: they share the process-wide lock and the sudo log.
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

fn new_manager() -> SudoManager {
    fake_sudo_dir();
    SudoManager::new().expect("create sudo manager")
}

fn sudo_k_count() -> usize {
    fs::read_to_string(fake_sudo_dir().join("sudo.log"))
        .map(|log| log.lines().count())
        .unwrap_or(0)
}

fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Whatever a reader connecting now would receive without blocking.
fn readable_now(manager: &SudoManager) -> Vec<u8> {
    let mut reader = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(manager.fifo_path())
        .expect("open fifo");
    thread::sleep(Duration::from_millis(100));
    let mut data = Vec::new();
    let _ = reader.read_to_end(&mut data);
    data
}

fn run(manager: &SudoManager, argv: &[&str]) -> io::Result<ExitStatus> {
    let argv: Vec<&OsStr> = argv.iter().map(OsStr::new).collect();
    manager.run_privileged(&argv)
}

#[test]
fn scripts_and_fifo_match_the_original() {
    let _serial = serial();
    let manager = new_manager();
    let temp = std::env::temp_dir();
    let name = |path: &Path| path.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(manager.askpass_path().parent(), Some(temp.as_path()));
    assert!(name(manager.askpass_path()).starts_with("linexin-askpass-"));
    assert!(name(manager.wrapper_path()).starts_with("linexin-sudo-"));
    let fifo_dir = manager.fifo_path().parent().unwrap();
    assert!(name(fifo_dir).starts_with("linexin-pipe-"));
    assert_eq!(name(manager.fifo_path()), "password_pipe");

    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(manager.askpass_path()), 0o700);
    assert_eq!(mode(manager.wrapper_path()), 0o700);
    assert_eq!(mode(fifo_dir), 0o700);
    assert_eq!(mode(manager.fifo_path()), 0o600);
    assert!(
        fs::metadata(manager.fifo_path())
            .unwrap()
            .file_type()
            .is_fifo()
    );

    assert_eq!(
        fs::read_to_string(manager.askpass_path()).unwrap(),
        format!("#!/bin/sh\ncat \"{}\"\n", manager.fifo_path().display())
    );
    assert_eq!(
        fs::read_to_string(manager.wrapper_path()).unwrap(),
        format!(
            "#!/bin/sh\nexport SUDO_ASKPASS='{}'\nexec sudo -A \"$@\"\n",
            manager.askpass_path().display()
        )
    );

    let paths = [
        manager.askpass_path().to_owned(),
        manager.wrapper_path().to_owned(),
        manager.fifo_path().to_owned(),
        fifo_dir.to_owned(),
    ];
    manager.cleanup();
    for path in &paths {
        assert!(!path.exists(), "{} survived cleanup", path.display());
    }
    manager.cleanup();
}

#[test]
fn shell_quoting_survives_hostile_paths() {
    let _serial = serial();
    let path = OsStr::new("/tmp/a'b\"c$d`e\\f");
    let script = [
        b"printf %s ".as_slice(),
        &single_quote(path),
        b" ",
        &double_quote(path),
    ]
    .concat();
    let output = Command::new("sh")
        .arg("-c")
        .arg(OsStr::from_bytes(&script))
        .output()
        .unwrap();
    assert_eq!(output.stdout, [path.as_bytes(), path.as_bytes()].concat());
}

#[test]
fn one_shot_feed_supplies_the_password_once() {
    let _serial = serial();
    let manager = new_manager();
    assert_eq!(
        run(&manager, &["true"]).unwrap_err().to_string(),
        "No password set"
    );
    manager.set_password(PASSWORD);
    assert!(run(&manager, &["true"]).unwrap().success());
    assert_eq!(
        run(&manager, &["sh", "-c", "exit 3"]).unwrap().code(),
        Some(3)
    );
    {
        let state = manager.shared.lock();
        assert!(!state.one_shot && !state.granted);
    }
    assert!(readable_now(&manager).is_empty());

    manager.set_password("wrong");
    assert_eq!(run(&manager, &["true"]).unwrap().code(), Some(1));
    manager.cleanup();
}

#[test]
fn drain_revokes_a_feed_the_command_did_not_consume() {
    let _serial = serial();
    let manager = new_manager();
    manager.set_password(PASSWORD);
    assert!(run(&manager, &["--cached", "true"]).unwrap().success());
    wait_until("the feeder to leave open()", || {
        !manager.shared.lock().feeder_waiting
    });
    assert!(!manager.shared.lock().one_shot);
    assert!(
        readable_now(&manager).is_empty(),
        "a revoked feed was served"
    );
    manager.cleanup();
}

#[test]
fn session_feeds_until_stopped() {
    let _serial = serial();
    let manager = new_manager();
    manager.start_session();
    assert!(!manager.shared.lock().session, "a session needs a password");

    manager.set_password(PASSWORD);
    manager.start_session();
    for _ in 0..3 {
        let status = Command::new(manager.wrapper_path())
            .arg("true")
            .status()
            .unwrap();
        assert!(status.success());
    }
    manager.stop_session();
    wait_until("the feeder to leave open()", || {
        !manager.shared.lock().feeder_waiting
    });
    assert!(
        readable_now(&manager).is_empty(),
        "a stopped session was served"
    );
    manager.cleanup();
}

#[test]
fn forget_password_wipes_it_and_clears_the_cache() {
    let _serial = serial();
    let manager = new_manager();
    manager.set_password(PASSWORD);
    assert_eq!(manager.password().as_deref(), Some(PASSWORD));
    let before = sudo_k_count();
    manager.forget_password();
    assert!(!manager.has_password());
    assert_eq!(manager.password(), None);
    assert_eq!(sudo_k_count(), before + 1);

    manager.set_password(PASSWORD);
    manager.set_password("");
    assert!(!manager.has_password(), "an empty password clears it");
    manager.cleanup();
}

#[test]
fn validate_password_uses_sudo_stdin() {
    let _serial = serial();
    let manager = new_manager();
    let before = sudo_k_count();
    assert!(manager.validate_password(PASSWORD));
    assert!(!manager.validate_password("wrong"));
    assert!(!manager.validate_password(""));
    assert_eq!(
        sudo_k_count(),
        before + 2,
        "each validation starts with sudo -k"
    );
    assert!(
        !manager.has_password(),
        "validation does not store the password"
    );
    manager.cleanup();
}

#[test]
fn cleanup_stops_a_feeder_blocked_in_open() {
    let _serial = serial();
    let manager = Arc::new(new_manager());
    manager.set_password(PASSWORD);
    manager.allow_feed();
    wait_until("the feeder to block in open()", || {
        manager.shared.lock().feeder_waiting
    });
    let (sender, receiver) = mpsc::channel();
    let cleaning = Arc::clone(&manager);
    thread::spawn(move || {
        cleaning.cleanup();
        let _ = sender.send(());
    });
    receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("cleanup did not stop the feeder");
    assert!(!manager.has_password());
    assert!(!manager.fifo_path().exists());
    manager.cleanup();
}

#[test]
fn concurrent_run_privileged_each_get_their_feed() {
    let _serial = serial();
    let manager = Arc::new(new_manager());
    manager.set_password(PASSWORD);
    let workers: Vec<_> = (0..4)
        .map(|code| {
            let manager = Arc::clone(&manager);
            thread::spawn(move || {
                let script = format!("sleep 0.{code}; exit {code}");
                run(&manager, &["sh", "-c", &script]).unwrap().code()
            })
        })
        .collect();
    let codes: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(codes, [Some(0), Some(1), Some(2), Some(3)]);
    let state = manager.shared.lock();
    assert!(!state.one_shot && !state.granted);
    drop(state);
    assert!(!lock::is_locked());
    manager.cleanup();
}

#[test]
fn run_privileged_holds_the_ui_lock() {
    let _serial = serial();
    let manager = Arc::new(new_manager());
    manager.set_password(PASSWORD);
    let running = Arc::clone(&manager);
    let worker = thread::spawn(move || run(&running, &["sleep", "0.5"]).unwrap().success());
    wait_until("the UI lock", lock::is_locked);
    assert!(worker.join().unwrap());
    assert!(!lock::is_locked());
    manager.cleanup();
}

#[test]
fn wrapper_command_detection() {
    let _serial = serial();
    let manager = new_manager();
    let wrapper = manager.wrapper_path().to_str().unwrap().to_owned();
    let name = manager
        .wrapper_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    assert!(manager.is_wrapper_command(&wrapper));
    assert!(manager.is_wrapper_command(&format!("/elsewhere/{name}")));
    assert!(manager.is_wrapper_command(&format!("{wrapper} pacman -Syu --noconfirm")));
    assert!(manager.is_wrapper_command(&format!("env A=1 {wrapper} true")));
    assert!(!manager.is_wrapper_command("sudo"));
    assert!(!manager.is_wrapper_command(""));
    assert!(!SudoManager::unavailable().is_wrapper_command(""));
    assert!(!SudoManager::unavailable().is_wrapper_command("ls"));
    manager.cleanup();
}

#[test]
fn lock_counts_and_applies_transitions_on_the_main_context() {
    let _serial = serial();
    let context = glib::MainContext::default();
    // Transitions scheduled by earlier tests must not reach the callback.
    while context.pending() {
        context.iteration(false);
    }
    let main_thread = thread::current().id();
    let applied = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&applied);
    lock::set_apply_callback(move |locked| {
        record.borrow_mut().push((locked, thread::current().id()));
    });

    thread::spawn(|| {
        lock::lock();
        lock::lock();
        assert!(lock::is_locked());
        lock::unlock();
        assert!(lock::is_locked());
        lock::unlock();
        lock::unlock();
        assert!(!lock::is_locked());
    })
    .join()
    .unwrap();
    assert!(applied.borrow().is_empty(), "the callback must be deferred");
    while context.pending() {
        context.iteration(false);
    }
    assert_eq!(
        *applied.borrow(),
        [(true, main_thread), (false, main_thread)]
    );

    lock::lock();
    assert_eq!(
        applied.borrow().len(),
        2,
        "lock() never applies synchronously"
    );
    lock::unlock();
    while context.pending() {
        context.iteration(false);
    }
    assert_eq!(applied.borrow().len(), 4);
}
