//! Host services shared by the Center UI, native widgets (through the plugin
//! ABI) and legacy Python widgets (through the embedded bridge).

/// Port of the original `CommandLockManager`: a counted lock that, while held,
/// makes the sidebar insensitive, hides the window controls and blocks closing.
pub mod lock {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Mutex, PoisonError};

    use gtk::glib;

    type ApplyCallback = Rc<dyn Fn(bool)>;

    static COUNT: Mutex<usize> = Mutex::new(0);

    thread_local! {
        static APPLY: RefCell<Option<ApplyCallback>> = const { RefCell::new(None) };
    }

    /// Register the GTK-main-thread callback that applies (`true`) or removes
    /// (`false`) the lock in the UI. Must be called on the GTK main thread; a
    /// later registration replaces the earlier one.
    pub fn set_apply_callback(callback: impl Fn(bool) + 'static) {
        APPLY.with(|apply| *apply.borrow_mut() = Some(Rc::new(callback)));
    }

    /// Increment the lock count; on the 0 → 1 transition schedule
    /// `callback(true)` on the GTK main context. Callable from any thread.
    pub fn lock() {
        let mut count = COUNT.lock().unwrap_or_else(PoisonError::into_inner);
        *count += 1;
        if *count == 1 {
            schedule(true);
        }
    }

    /// Decrement the lock count (saturating); on 1 → 0 schedule
    /// `callback(false)` on the GTK main context. Callable from any thread.
    pub fn unlock() {
        let mut count = COUNT.lock().unwrap_or_else(PoisonError::into_inner);
        if *count == 0 {
            return;
        }
        *count -= 1;
        if *count == 0 {
            schedule(false);
        }
    }

    /// Whether the lock count is non-zero.
    pub fn is_locked() -> bool {
        *COUNT.lock().unwrap_or_else(PoisonError::into_inner) > 0
    }

    // Always deferred like the original `GLib.idle_add`, and scheduled while
    // the count is held so transitions reach the UI in order.
    fn schedule(locked: bool) {
        glib::idle_add_once(move || {
            let apply = APPLY.with(|apply| apply.borrow().clone());
            if let Some(apply) = apply {
                apply(locked);
            }
        });
    }
}

/// Port of the original `SudoManager` (one process-wide instance shared by the
/// host's own actions, native widgets and Python widgets' `sudo_manager`).
///
/// On creation it writes, with mode 0700 in private temporary locations, an
/// askpass script (`cat <fifo>`) and a wrapper script
/// (`SUDO_ASKPASS=<askpass> exec sudo -A "$@"`), creates the FIFO (0600) and
/// starts a feeder thread that writes the stored password into the FIFO only
/// while feeds are allowed (one per `allow_feed`, unlimited during a session).
pub mod sudo {
    use std::ffi::{CString, OsStr, OsString};
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitStatus, Stdio};
    use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    pub struct SudoManager {
        wrapper: PathBuf,
        askpass: PathBuf,
        fifo_dir: PathBuf,
        shared: Arc<Shared>,
        feeder: Mutex<Option<JoinHandle<()>>>,
    }

    /// State shared with the feeder thread.
    struct Shared {
        fifo: PathBuf,
        state: Mutex<State>,
        changed: Condvar,
    }

    #[derive(Default)]
    struct State {
        password: Option<Secret>,
        /// An `allow_feed` caller has not drained yet. One-shot grants are
        /// serialized: every reader of the shared FIFO receives whatever is
        /// written, so two pending one-shot feeds could reach one askpass.
        granted: bool,
        /// The granted feed has not been consumed yet.
        one_shot: bool,
        session: bool,
        /// The feeder is blocked opening the FIFO for writing.
        feeder_waiting: bool,
        stopped: bool,
    }

    impl State {
        fn feed_allowed(&self) -> bool {
            self.session || self.one_shot
        }
    }

    /// Bytes that are overwritten with zeros when dropped.
    struct Secret(Vec<u8>);

    impl Secret {
        fn line(bytes: &[u8]) -> Self {
            let mut line = Vec::with_capacity(bytes.len() + 1);
            line.extend_from_slice(bytes);
            line.push(b'\n');
            Self(line)
        }
    }

    impl Drop for Secret {
        fn drop(&mut self) {
            wipe(&mut self.0);
        }
    }

    fn wipe(bytes: &mut [u8]) {
        for byte in bytes.iter_mut() {
            // SAFETY: `byte` is a valid, exclusive reference.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }

    static MANAGER: OnceLock<SudoManager> = OnceLock::new();

    const READER_GRACE: Duration = Duration::from_millis(50);

    /// The shared instance, created on first use.
    pub fn manager() -> &'static SudoManager {
        MANAGER.get_or_init(|| {
            SudoManager::new().unwrap_or_else(|error| {
                eprintln!("Cannot set up the sudo helper: {error}");
                SudoManager::unavailable()
            })
        })
    }

    /// The shared instance only if it already exists (original
    /// `SudoManager._instance`), e.g. for lock detection.
    pub fn existing() -> Option<&'static SudoManager> {
        MANAGER.get()
    }

    impl SudoManager {
        fn new() -> io::Result<Self> {
            let temp = std::env::temp_dir();
            let mut created = Vec::new();
            let result = Self::create(&temp, &mut created);
            if result.is_err() {
                for path in created.iter().rev() {
                    let _ = fs::remove_file(path).or_else(|_| fs::remove_dir(path));
                }
            }
            result
        }

        fn create(temp: &Path, created: &mut Vec<PathBuf>) -> io::Result<Self> {
            let (askpass, askpass_file) = create_temp_file(temp, "linexin-askpass-")?;
            created.push(askpass.clone());
            let (wrapper, wrapper_file) = create_temp_file(temp, "linexin-sudo-")?;
            created.push(wrapper.clone());
            let fifo_dir = create_temp_dir(temp, "linexin-pipe-")?;
            created.push(fifo_dir.clone());
            let fifo = fifo_dir.join("password_pipe");
            make_fifo(&fifo)?;
            created.push(fifo.clone());

            let mut askpass_script = b"#!/bin/sh\ncat ".to_vec();
            askpass_script.extend(double_quote(fifo.as_os_str()));
            askpass_script.push(b'\n');
            write_script(askpass_file, &askpass_script)?;
            let mut wrapper_script = b"#!/bin/sh\nexport SUDO_ASKPASS=".to_vec();
            wrapper_script.extend(single_quote(askpass.as_os_str()));
            wrapper_script.extend_from_slice(b"\nexec sudo -A \"$@\"\n");
            write_script(wrapper_file, &wrapper_script)?;

            let shared = Arc::new(Shared {
                fifo,
                state: Mutex::new(State::default()),
                changed: Condvar::new(),
            });
            let feeder_shared = Arc::clone(&shared);
            let feeder = thread::Builder::new()
                .name("linexin-sudo-feeder".into())
                .spawn(move || feed_pipe_loop(&feeder_shared))?;
            Ok(Self {
                wrapper,
                askpass,
                fifo_dir,
                shared,
                feeder: Mutex::new(Some(feeder)),
            })
        }

        /// Inert instance used when the temporary files cannot be created:
        /// validation still works, privileged runs fail to start.
        fn unavailable() -> Self {
            Self {
                wrapper: PathBuf::new(),
                askpass: PathBuf::new(),
                fifo_dir: PathBuf::new(),
                shared: Arc::new(Shared {
                    fifo: PathBuf::new(),
                    state: Mutex::new(State {
                        stopped: true,
                        ..State::default()
                    }),
                    changed: Condvar::new(),
                }),
                feeder: Mutex::new(None),
            }
        }

        pub fn wrapper_path(&self) -> &Path {
            &self.wrapper
        }
        pub fn askpass_path(&self) -> &Path {
            &self.askpass
        }
        pub fn fifo_path(&self) -> &Path {
            &self.shared.fifo
        }

        /// `sudo -k`, then `sudo -S -v` with the password on stdin and
        /// `LC_ALL=C`; true on exit status 0. Blocking.
        pub fn validate_password(&self, password: &str) -> bool {
            if password.is_empty() {
                return false;
            }
            self.clear_cache();
            let child = Command::new("sudo")
                .args(["-S", "-v"])
                .env("LC_ALL", "C")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            let mut child = match child {
                Ok(child) => child,
                Err(error) => {
                    eprintln!("Sudo validation error: {error}");
                    return false;
                }
            };
            if let Some(mut stdin) = child.stdin.take() {
                let input = Secret::line(password.as_bytes());
                // sudo may exit without reading (e.g. no tty rules); its status decides.
                let _ = stdin.write_all(&input.0);
            }
            match child.wait_with_output() {
                Ok(output) => output.status.success(),
                Err(error) => {
                    eprintln!("Sudo validation error: {error}");
                    false
                }
            }
        }

        /// Store the password; an empty one clears (and wipes) it.
        pub fn set_password(&self, password: &str) {
            let mut state = self.shared.lock();
            state.password = (!password.is_empty()).then(|| Secret(password.as_bytes().to_vec()));
            drop(state);
            self.shared.changed.notify_all();
        }
        pub fn password(&self) -> Option<String> {
            let state = self.shared.lock();
            let password = state.password.as_ref()?;
            Some(String::from_utf8_lossy(&password.0).into_owned())
        }
        pub fn has_password(&self) -> bool {
            self.shared.lock().password.is_some()
        }
        /// Clear the password (wiping memory) and run `sudo -k`.
        pub fn forget_password(&self) {
            self.shared.lock().password = None;
            self.clear_cache();
        }
        /// `sudo -k`.
        pub fn clear_cache(&self) {
            let _ = Command::new("sudo").arg("-k").status();
        }
        /// Allow one more password feed (first half of `run_privileged`).
        /// Waits while another caller's grant is still pending.
        pub fn allow_feed(&self) {
            let mut state = self
                .shared
                .changed
                .wait_while(self.shared.lock(), |state| state.granted && !state.stopped)
                .unwrap_or_else(PoisonError::into_inner);
            state.granted = true;
            state.one_shot = true;
            drop(state);
            self.shared.changed.notify_all();
        }
        /// Revoke unconsumed feeds and drain the FIFO if a feed is pending
        /// (second half of `run_privileged`, original `_drain_pipe`).
        pub fn drain(&self) {
            let mut state = self.shared.lock();
            state.granted = false;
            state.one_shot = false;
            let unblock = state.feeder_waiting && !state.feed_allowed();
            drop(state);
            self.shared.changed.notify_all();
            if unblock {
                drop(self.shared.open_reader());
            }
        }
        /// Original `start_privileged_session` (no-op without a password).
        pub fn start_session(&self) {
            let mut state = self.shared.lock();
            if state.password.is_none() {
                return;
            }
            state.session = true;
            drop(state);
            self.shared.changed.notify_all();
        }
        /// Original `stop_privileged_session`.
        pub fn stop_session(&self) {
            let mut state = self.shared.lock();
            state.session = false;
            let unblock = state.feeder_waiting && !state.feed_allowed();
            drop(state);
            if unblock {
                drop(self.shared.open_reader());
            }
        }

        /// Lock the UI, allow one feed, run `wrapper argv...` (inheriting
        /// stdio and environment), drain, unlock. Errors when no password is
        /// stored. Blocking.
        pub fn run_privileged(&self, argv: &[&OsStr]) -> io::Result<ExitStatus> {
            if !self.has_password() {
                return Err(io::Error::other("No password set"));
            }
            crate::lock::lock();
            self.allow_feed();
            let status = Command::new(&self.wrapper).args(argv).status();
            self.drain();
            crate::lock::unlock();
            status
        }

        /// Original `_should_lock_command` test for one executable/command
        /// string: equals the wrapper path, ends with its file name, or (for a
        /// shell command string) contains the wrapper path.
        pub fn is_wrapper_command(&self, command: &str) -> bool {
            let wrapper = self.wrapper.as_os_str().as_bytes();
            let Some(name) = self.wrapper.file_name() else {
                return false;
            };
            let command = command.as_bytes();
            command == wrapper
                || command.ends_with(name.as_bytes())
                || command.windows(wrapper.len()).any(|part| part == wrapper)
        }

        /// Stop the feeder, forget the password, remove scripts/FIFO/dirs.
        /// Idempotent; called at application shutdown.
        pub fn cleanup(&self) {
            // Held throughout so concurrent calls run one after another.
            let mut feeder = self.feeder.lock().unwrap_or_else(PoisonError::into_inner);
            let mut state = self.shared.lock();
            if state.stopped {
                return;
            }
            state.stopped = true;
            state.session = false;
            state.one_shot = false;
            state.password = None;
            drop(state);
            self.shared.changed.notify_all();
            if let Some(feeder) = feeder.take() {
                // An open read end lets a pending (or imminent) blocking open
                // in the feeder return, so it can see the stop and exit.
                if let Ok(reader) = self.shared.open_reader() {
                    let _ = feeder.join();
                    drop(reader);
                }
            }
            self.clear_cache();
            for path in [&self.askpass, &self.wrapper, &self.shared.fifo] {
                let _ = fs::remove_file(path);
            }
            let _ = fs::remove_dir(&self.fifo_dir);
        }
    }

    impl Shared {
        fn lock(&self) -> MutexGuard<'_, State> {
            self.state.lock().unwrap_or_else(PoisonError::into_inner)
        }

        /// Non-blocking reader whose open releases a writer blocked in open();
        /// anything written meanwhile is read and discarded.
        fn open_reader(&self) -> io::Result<File> {
            let mut reader = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&self.fifo)?;
            let mut discarded = [0u8; 1024];
            let _ = reader.read(&mut discarded);
            wipe(&mut discarded);
            Ok(reader)
        }
    }

    fn feed_pipe_loop(shared: &Shared) {
        block_sigpipe();
        let mut state = shared.lock();
        loop {
            state = shared
                .changed
                .wait_while(state, |state| {
                    !state.stopped && !(state.feed_allowed() && state.password.is_some())
                })
                .unwrap_or_else(PoisonError::into_inner);
            if state.stopped {
                return;
            }
            state.feeder_waiting = true;
            drop(state);
            // Blocks until a reader (askpass `cat`, drain or cleanup) connects.
            let pipe = OpenOptions::new().write(true).open(&shared.fifo);
            state = shared.lock();
            state.feeder_waiting = false;
            if state.stopped {
                return;
            }
            let mut pipe = match pipe {
                Ok(pipe) => pipe,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    eprintln!("Pipe error: {error}");
                    state = shared
                        .changed
                        .wait_timeout(state, Duration::from_secs(1))
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                    continue;
                }
            };
            // Re-checked after connecting: a revoked feed or forgotten
            // password closes the pipe without writing.
            let line = match &state.password {
                Some(password) if state.feed_allowed() => Secret::line(&password.0),
                _ => continue,
            };
            drop(state);
            let written = pipe.write_all(&line.0).is_ok();
            drop(line);
            drop(pipe);
            state = shared.lock();
            if written {
                state.one_shot = false;
                // Let the reader see end-of-file before connecting again: a
                // reader still holding the FIFO would be fed a second time.
                state = shared
                    .changed
                    .wait_timeout_while(state, READER_GRACE, |state| !state.stopped)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }
    }

    /// Writes to a FIFO whose reader went away must fail with EPIPE instead of
    /// killing the process.
    fn block_sigpipe() {
        // SAFETY: plain signal-mask manipulation of the calling thread.
        unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, libc::SIGPIPE);
            libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        }
    }

    fn template(directory: &Path, prefix: &str) -> io::Result<CString> {
        let mut path = directory.join(prefix).into_os_string().into_vec();
        path.extend_from_slice(b"XXXXXX");
        CString::new(path).map_err(io::Error::other)
    }

    fn template_path(template: CString) -> PathBuf {
        PathBuf::from(OsString::from_vec(template.into_bytes()))
    }

    /// Like Python's `NamedTemporaryFile(delete=False, prefix=...)`: a new 0600 file.
    fn create_temp_file(directory: &Path, prefix: &str) -> io::Result<(PathBuf, File)> {
        let template = template(directory, prefix)?.into_raw();
        // SAFETY: `template` is a writable NUL-terminated buffer owned below.
        let fd = unsafe { libc::mkostemp(template, libc::O_CLOEXEC) };
        let error = io::Error::last_os_error();
        // SAFETY: reclaims the buffer released by `into_raw`.
        let template = unsafe { CString::from_raw(template) };
        if fd < 0 {
            return Err(error);
        }
        // SAFETY: `fd` is a freshly opened descriptor owned by nobody else.
        let file = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        Ok((template_path(template), file))
    }

    /// Like Python's `tempfile.mkdtemp(prefix=...)`: a new 0700 directory.
    fn create_temp_dir(directory: &Path, prefix: &str) -> io::Result<PathBuf> {
        let template = template(directory, prefix)?.into_raw();
        // SAFETY: `template` is a writable NUL-terminated buffer owned below.
        let created = unsafe { libc::mkdtemp(template) };
        let error = io::Error::last_os_error();
        // SAFETY: reclaims the buffer released by `into_raw`.
        let template = unsafe { CString::from_raw(template) };
        if created.is_null() {
            return Err(error);
        }
        Ok(template_path(template))
    }

    fn make_fifo(path: &Path) -> io::Result<()> {
        let path = CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
        // SAFETY: valid NUL-terminated path.
        if unsafe { libc::mkfifo(path.as_ptr(), 0o600) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Writes and closes the script (a writer left open would make exec fail
    /// with ETXTBSY).
    fn write_script(mut file: File, contents: &[u8]) -> io::Result<()> {
        file.write_all(contents)?;
        file.set_permissions(fs::Permissions::from_mode(0o700))
    }

    /// POSIX shell double-quoted word (the original askpass quoting).
    fn double_quote(value: &OsStr) -> Vec<u8> {
        let mut quoted = vec![b'"'];
        for &byte in value.as_bytes() {
            if matches!(byte, b'"' | b'\\' | b'$' | b'`') {
                quoted.push(b'\\');
            }
            quoted.push(byte);
        }
        quoted.push(b'"');
        quoted
    }

    /// POSIX shell single-quoted word (the original wrapper quoting).
    fn single_quote(value: &OsStr) -> Vec<u8> {
        let mut quoted = vec![b'\''];
        for &byte in value.as_bytes() {
            if byte == b'\'' {
                quoted.extend_from_slice(b"'\\''");
            } else {
                quoted.push(byte);
            }
        }
        quoted.push(b'\'');
        quoted
    }

    #[cfg(test)]
    mod tests;
}
