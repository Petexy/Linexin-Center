//! Data collection of the original widget's worker thread (`load_info`), with
//! the same sources, commands, timeouts and fallbacks. psutil values are read
//! from /proc and statvfs the way psutil 7 computes them.

use std::collections::HashMap;
use std::ffi::CStr;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::distro;

/// What `update_ui` shows. Fields `load_info` did not reach keep the defaults
/// `update_ui` falls back to; version rows are skipped when `None`.
#[derive(Clone, Debug)]
pub struct SystemInfo {
    pub os_name: String,
    pub version_id: Option<String>,
    pub version_date: Option<String>,
    pub kernel: String,
    pub uptime: String,
    pub session_type: String,
    pub desktop_env: String,
    pub window_manager: String,
    pub cpu_info: String,
    pub cpu_count: Option<libc::c_long>,
    pub gpu_info: String,
    pub memory: Usage,
    pub disk: Usage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    pub used: i64,
    pub total: i64,
    pub percent: f64,
}

const DESKTOP_NAMES: &[(&str, &str)] = &[
    ("gnome", "GNOME"),
    ("kde", "KDE"),
    ("xfce", "Xfce"),
    ("mate", "MATE"),
    ("cinnamon", "Cinnamon"),
    ("lxde", "LXDE"),
    ("lxqt", "LXQt"),
    ("pantheon", "Pantheon"),
    ("budgie", "Budgie"),
    ("deepin", "Deepin"),
    ("unity", "Unity"),
    ("i3", "i3"),
    ("sway", "Sway"),
    ("awesome", "Awesome"),
    ("openbox", "Openbox"),
    ("fluxbox", "Fluxbox"),
    ("bspwm", "bspwm"),
    ("dwm", "dwm"),
    ("qtile", "Qtile"),
    ("herbstluftwm", "herbstluftwm"),
];

// Checked in this order as substrings of the whole `ps aux` output.
const WINDOW_MANAGERS: &[(&str, &str)] = &[
    ("mutter", "Mutter"),
    ("kwin_x11", "KWin"),
    ("kwin_wayland", "KWin"),
    ("kwin", "KWin"),
    ("xfwm4", "Xfwm4"),
    ("openbox", "Openbox"),
    ("i3", "i3"),
    ("sway", "Sway"),
    ("awesome", "Awesome"),
    ("dwm", "DWM"),
    ("bspwm", "bspwm"),
    ("qtile", "Qtile"),
    ("herbstluftwm", "herbstluftwm"),
    ("fluxbox", "Fluxbox"),
    ("marco", "Marco"),
    ("metacity", "Metacity"),
    ("compiz", "Compiz"),
    ("enlightenment", "Enlightenment"),
    ("cwm", "CWM"),
    ("jwm", "JWM"),
];

/// Gather everything like the original `load_info`. `unknown` is the translated
/// "Unknown". Returns `None` when the OS name could not be read (the original
/// then shows only an error row).
pub fn collect(unknown: &str) -> Option<SystemInfo> {
    let os_name = match distro::os_name() {
        Ok(name) => name,
        Err(error) => {
            eprintln!("Error loading system info: {error}");
            return None;
        }
    };
    let mut info = SystemInfo {
        os_name,
        version_id: None,
        version_date: None,
        kernel: unknown.to_owned(),
        uptime: unknown.to_owned(),
        session_type: unknown.to_owned(),
        desktop_env: unknown.to_owned(),
        window_manager: unknown.to_owned(),
        cpu_info: unknown.to_owned(),
        cpu_count: None,
        gpu_info: unknown.to_owned(),
        memory: Usage::default(),
        disk: Usage::default(),
    };
    if let Err(error) = info.fill(unknown) {
        eprintln!("Error loading system info: {error}");
    }
    Some(info)
}

impl SystemInfo {
    /// The remaining steps of `load_info`, in its order. `Err` stands for an
    /// exception the original does not catch, which skips every later step.
    fn fill(&mut self, unknown: &str) -> Result<(), String> {
        let or_unknown = |value: Option<String>| value.unwrap_or_else(|| unknown.to_owned());
        self.version_id = version_id();
        self.version_date = version_date();
        self.kernel = or_unknown(uname().map(|uname| uname.release));
        self.uptime = or_unknown(uptime());
        self.session_type = or_unknown(session_type());
        self.desktop_env = or_unknown(desktop_environment());
        self.window_manager = or_unknown(window_manager()?);
        self.cpu_info = or_unknown(cpu_info());
        self.cpu_count = cpu_count();
        self.gpu_info = gpu_info(unknown)?;
        self.memory = virtual_memory()?;
        self.disk = disk_usage()?;
        Ok(())
    }
}

/// Original `format_bytes`.
pub fn format_bytes(bytes: i64) -> String {
    if bytes < 1024 {
        return format!("{:.1} B", bytes as f64);
    }
    let mut value = bytes as f64 / 1024.0;
    for unit in ["KB", "MB", "GB", "TB"] {
        if value < 1024.0 {
            return format!("{value:.1} {unit}");
        }
        value /= 1024.0;
    }
    format!("{value:.1} PB")
}

pub struct Uname {
    pub nodename: String,
    pub release: String,
}

/// `os.uname()`.
pub fn uname() -> Option<Uname> {
    // SAFETY: utsname is plain data; uname fills it with NUL-terminated fields.
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut name) } != 0 {
        return None;
    }
    let field = |chars: &[libc::c_char]| {
        // SAFETY: uname NUL-terminates every field within its array.
        unsafe { CStr::from_ptr(chars.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    };
    Some(Uname {
        nodename: field(&name.nodename),
        release: field(&name.release),
    })
}

/// Python's text-mode `open(path).read()`: strict UTF-8, universal newlines.
pub fn read_text(path: impl AsRef<Path>) -> io::Result<String> {
    fs::read_to_string(path).map(|text| universal_newlines(&text))
}

pub fn universal_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn env(key: &str) -> String {
    std::env::var_os(key)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Python's `str.capitalize()`.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first
            .to_uppercase()
            .chain(chars.as_str().to_lowercase().chars())
            .collect()
    })
}

fn version_id() -> Option<String> {
    let text = read_text("/etc/os-release").ok()?;
    let line = text.lines().find(|line| line.starts_with("VERSION_ID="))?;
    let value = line.split('=').nth(1)?;
    Some(value.trim().trim_matches('"').to_owned()).filter(|value| !value.is_empty())
}

fn version_date() -> Option<String> {
    let text = read_text("/version").ok()?;
    Some(text.trim().to_owned()).filter(|text| !text.is_empty())
}

fn uptime() -> Option<String> {
    let boot_time = boot_time()?;
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_secs_f64(),
        Err(error) => -error.duration().as_secs_f64(),
    };
    Some(format_uptime(now - boot_time))
}

/// `psutil.boot_time()`: the `btime` line of /proc/stat. Values the original
/// cannot turn into days, hours and minutes give `None` ("Unknown") as well.
fn boot_time() -> Option<f64> {
    let stat = fs::read("/proc/stat").ok()?;
    let stat = String::from_utf8_lossy(&stat);
    let line = stat.lines().find(|line| line.starts_with("btime"))?;
    let boot_time: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    boot_time.is_finite().then_some(boot_time)
}

fn format_uptime(uptime: f64) -> String {
    let days = py_divmod(uptime, 86400.0).0 as i64;
    let hours = py_divmod(py_divmod(uptime, 86400.0).1, 3600.0).0 as i64;
    let minutes = py_divmod(py_divmod(uptime, 3600.0).1, 60.0).0 as i64;
    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

/// Python's float `(x // y, x % y)`.
fn py_divmod(x: f64, y: f64) -> (f64, f64) {
    let mut modulo = x % y;
    let mut div = (x - modulo) / y;
    if modulo != 0.0 {
        if (y < 0.0) != (modulo < 0.0) {
            modulo += y;
            div -= 1.0;
        }
    } else {
        modulo = 0.0_f64.copysign(y);
    }
    let floor_div = if div != 0.0 {
        let floor = div.floor();
        if div - floor > 0.5 {
            floor + 1.0
        } else {
            floor
        }
    } else {
        0.0_f64.copysign(x / y)
    };
    (floor_div, modulo)
}

fn session_type() -> Option<String> {
    let session = env("XDG_SESSION_TYPE").to_lowercase();
    (!session.is_empty()).then(|| capitalize(&session))
}

fn desktop_environment() -> Option<String> {
    [
        "XDG_CURRENT_DESKTOP",
        "DESKTOP_SESSION",
        "XDG_SESSION_DESKTOP",
    ]
    .into_iter()
    .map(|key| env(key).to_lowercase())
    .find(|desktop| !desktop.is_empty())
    .map(|desktop| {
        DESKTOP_NAMES
            .iter()
            .find(|(key, _)| *key == desktop)
            .map_or_else(|| capitalize(&desktop), |(_, name)| (*name).to_owned())
    })
}

/// `WINDOW_MANAGER`, then the first known name in `ps aux`, then the X11
/// `_NET_WM_NAME` (except GNOME Shell's).
fn window_manager() -> Result<Option<String>, String> {
    let configured = env("WINDOW_MANAGER");
    if !configured.is_empty() {
        return Ok(configured.rsplit('/').next().map(str::to_owned));
    }
    if let Some(ps) = run(&["ps", "aux"], SHORT_TIMEOUT)?.filter(|ps| ps.success) {
        let found = WINDOW_MANAGERS
            .iter()
            .find(|(process, _)| ps.stdout.contains(process));
        if let Some((_, name)) = found {
            return Ok(Some((*name).to_owned()));
        }
    }
    if env("DISPLAY").is_empty() {
        return Ok(None);
    }
    let check = run(
        &["xprop", "-root", "_NET_SUPPORTING_WM_CHECK"],
        SHORT_TIMEOUT,
    )?;
    let Some(check) = check.filter(|r| r.success && r.stdout.contains("window id")) else {
        return Ok(None);
    };
    let id = check.stdout.split_whitespace().last().unwrap_or_default();
    let name = run(&["xprop", "-id", id, "_NET_WM_NAME"], SHORT_TIMEOUT)?;
    let Some(name) = name.filter(|r| r.success && r.stdout.contains('=')) else {
        return Ok(None);
    };
    let name = name.stdout.split('=').nth(1).unwrap_or_default();
    let name = name.trim().trim_matches('"');
    Ok((name.to_lowercase() != "gnome shell").then(|| name.to_owned()))
}

fn cpu_info() -> Option<String> {
    let cpuinfo = read_text("/proc/cpuinfo").ok()?;
    let line = cpuinfo
        .lines()
        .find(|line| line.starts_with("model name"))?;
    Some(line.split(':').nth(1)?.trim().to_owned())
}

/// `psutil.cpu_count()`: online logical CPUs.
fn cpu_count() -> Option<libc::c_long> {
    // SAFETY: sysconf has no preconditions.
    let count = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    (count >= 1).then_some(count)
}

/// `lspci` name plus the first driver found: nvidia-smi, then amdgpu, then i915.
fn gpu_info(unknown: &str) -> Result<String, String> {
    let gpu_name = run(&["lspci"], Duration::from_secs(10))?
        .filter(|lspci| lspci.success)
        .and_then(|lspci| parse_lspci(&lspci.stdout))
        .unwrap_or_else(|| unknown.to_owned());
    let nvidia_smi = [
        "nvidia-smi",
        "--query-gpu=driver_version",
        "--format=csv,noheader,nounits",
    ];
    let mut driver = run(&nvidia_smi, SHORT_TIMEOUT)?
        .filter(|r| r.success && !r.stdout.trim().is_empty())
        .map(|r| format!("NVIDIA {}", r.stdout.trim()));
    for (module, name) in [("amdgpu", "AMDGPU"), ("i915", "Intel i915")] {
        if driver.is_none() {
            driver = run(&["modinfo", module], SHORT_TIMEOUT)?
                .filter(|r| r.success)
                .map(|_| name.to_owned());
        }
    }
    Ok(match driver {
        Some(driver) => format!("{gpu_name} ({driver})"),
        None => gpu_name,
    })
}

/// GPU name from the first VGA/Display/3D line of `lspci`, cleaned up exactly
/// like the original (including taking the first `[...]` for NVIDIA and AMD).
fn parse_lspci(output: &str) -> Option<String> {
    let line = output.split('\n').find(|line| {
        (line.contains("VGA") || line.contains("Display") || line.contains("3D"))
            && line.contains(':')
    })?;
    let mut gpu = line.splitn(3, ':').last()?.trim().to_owned();
    for prefix in [
        "VGA compatible controller: ",
        "Display controller: ",
        "3D controller: ",
    ] {
        gpu = gpu.replace(prefix, "");
    }
    if let Some((before, _)) = gpu.split_once("(rev") {
        gpu = before.trim().to_owned();
    }
    let first_bracket = |gpu: String| match (gpu.find('['), gpu.find(']')) {
        (Some(open), Some(close)) => gpu.get(open + 1..close).unwrap_or_default().to_owned(),
        _ => gpu,
    };
    Some(if gpu.contains("NVIDIA Corporation") {
        first_bracket(gpu.replace("NVIDIA Corporation ", ""))
    } else if gpu.contains("AMD") || gpu.contains("Advanced Micro Devices") {
        first_bracket(
            gpu.replace("Advanced Micro Devices, Inc. ", "")
                .replace("AMD ", ""),
        )
    } else if gpu.contains("Intel") {
        gpu.replace("Intel Corporation ", "")
    } else {
        gpu
    })
}

/// `psutil.virtual_memory()`: used, total and percent.
fn virtual_memory() -> Result<Usage, String> {
    let meminfo = fs::read("/proc/meminfo").map_err(|e| e.to_string())?;
    let mems = parse_meminfo(&String::from_utf8_lossy(&meminfo))?;
    // SAFETY: sysconf has no preconditions.
    let page_size = || unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as f64;
    let zoneinfo = || {
        let zoneinfo = fs::read("/proc/zoneinfo").ok()?;
        Some(String::from_utf8_lossy(&zoneinfo).into_owned())
    };
    memory_usage(&mems, page_size, zoneinfo)
}

fn parse_meminfo(text: &str) -> Result<HashMap<String, i64>, String> {
    text.lines()
        .map(|line| {
            let mut fields = line.split_ascii_whitespace();
            let (Some(key), Some(value)) = (fields.next(), fields.next()) else {
                return Err(format!("malformed /proc/meminfo line: {line:?}"));
            };
            let value: i64 = value
                .parse()
                .map_err(|_| format!("invalid number {value:?}"))?;
            Ok((key.to_owned(), value.saturating_mul(1024)))
        })
        .collect()
}

fn memory_usage(
    mems: &HashMap<String, i64>,
    page_size: impl Fn() -> f64,
    zoneinfo: impl Fn() -> Option<String>,
) -> Result<Usage, String> {
    let field = |key: &str| mems.get(key).copied().ok_or_else(|| format!("'{key}'"));
    let total = field("MemTotal:")?;
    let free = field("MemFree:")?;
    let mut available = match mems.get("MemAvailable:") {
        Some(&available) if available != 0 => available,
        // Missing (kernel < 3.14) or reported as 0: psutil estimates it.
        _ => calculate_available(mems, free, page_size, zoneinfo)?,
    };
    if available < 0 {
        available = 0;
    } else if available > total {
        available = free;
    }
    Ok(Usage {
        used: total - available,
        total,
        percent: usage_percent(total - available, total),
    })
}

/// psutil's `calculate_avail_vmem`.
fn calculate_available(
    mems: &HashMap<String, i64>,
    free: i64,
    page_size: impl Fn() -> f64,
    zoneinfo: impl Fn() -> Option<String>,
) -> Result<i64, String> {
    let fallback = free + mems.get("Cached:").copied().unwrap_or(0);
    let (Some(&active_file), Some(&inactive_file), Some(&slab_reclaimable)) = (
        mems.get("Active(file):"),
        mems.get("Inactive(file):"),
        mems.get("SReclaimable:"),
    ) else {
        return Ok(fallback);
    };
    let Some(zoneinfo) = zoneinfo() else {
        return Ok(fallback);
    };
    let mut watermark_low = 0_i64;
    for line in zoneinfo
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("low"))
    {
        let value = line
            .split_whitespace()
            .nth(1)
            .ok_or("malformed /proc/zoneinfo")?;
        watermark_low += value
            .parse::<i64>()
            .map_err(|_| format!("invalid number {value:?}"))?;
    }
    let watermark_low = watermark_low as f64 * page_size();
    let pagecache = (active_file + inactive_file) as f64;
    let pagecache = pagecache - (pagecache / 2.0).min(watermark_low);
    let slab_reclaimable = slab_reclaimable as f64;
    let available = free as f64 - watermark_low
        + pagecache
        + (slab_reclaimable - (slab_reclaimable / 2.0).min(watermark_low));
    Ok(available as i64)
}

/// `psutil.disk_usage("/")`: percent relative to the space usable by users.
fn disk_usage() -> Result<Usage, String> {
    // SAFETY: statvfs is plain data that the call fills in.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: valid NUL-terminated path and output buffer.
    if unsafe { libc::statvfs(c"/".as_ptr(), &mut stat) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let bytes = |blocks: libc::fsblkcnt_t| {
        i64::try_from(u128::from(blocks) * u128::from(stat.f_frsize)).unwrap_or(i64::MAX)
    };
    Ok(disk_usage_from(
        bytes(stat.f_blocks),
        bytes(stat.f_bfree),
        bytes(stat.f_bavail),
    ))
}

fn disk_usage_from(total: i64, available_to_root: i64, available_to_user: i64) -> Usage {
    let used = total - available_to_root;
    Usage {
        used,
        total,
        percent: usage_percent(used, used + available_to_user),
    }
}

/// psutil's `usage_percent(used, total, round_=1)`.
fn usage_percent(used: i64, total: i64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let percent = used as f64 / total as f64 * 100.0;
    // Python's round(x, 1) is the correctly rounded shortest decimal, as here.
    format!("{percent:.1}").parse().unwrap_or(percent)
}

const SHORT_TIMEOUT: Duration = Duration::from_secs(5);

struct Completed {
    success: bool,
    stdout: String,
}

/// `subprocess.run(argv, capture_output=True, text=True, timeout=timeout)`.
/// `Ok(None)` is what the original catches: the program is missing or timed
/// out (it is killed). `Err` is any other exception: another spawn error or
/// output that is not UTF-8.
fn run(argv: &[&str], timeout: Duration) -> Result<Option<Completed>, String> {
    let deadline = Instant::now() + timeout;
    let (program, args) = argv.split_first().ok_or("empty command")?;
    let spawned = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{program}: {error}")),
    };
    let stdout = read_in_background(child.stdout.take());
    let stderr = read_in_background(child.stderr.take());
    let remaining = || deadline.saturating_duration_since(Instant::now());
    let output = stdout
        .recv_timeout(remaining())
        .ok()
        .zip(stderr.recv_timeout(remaining()).ok());
    let status = output.as_ref().and_then(|_| {
        loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                _ => break None,
            }
        }
    });
    let (Some((stdout, stderr)), Some(status)) = (output, status) else {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(None);
    };
    let text = |bytes| String::from_utf8(bytes).map_err(|error| format!("{program}: {error}"));
    let stdout = universal_newlines(&text(stdout)?);
    text(stderr)?;
    Ok(Some(Completed {
        success: status.success(),
        stdout,
    }))
}

fn read_in_background(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    if let Some(mut pipe) = pipe {
        // Without the thread the sender is dropped and the read counts as failed.
        let _ = thread::Builder::new()
            .name("system-information-pipe".into())
            .spawn(move || {
                let mut output = Vec::new();
                let _ = pipe.read_to_end(&mut output);
                let _ = sender.send(output);
            });
    }
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_are_formatted_like_python() {
        assert_eq!(format_bytes(0), "0.0 B");
        assert_eq!(format_bytes(1023), "1023.0 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(23_354_413_056), "21.8 GB");
        assert_eq!(format_bytes(32_706_076_672), "30.5 GB");
        assert_eq!(format_bytes(250_315_337_728), "233.1 GB");
        assert_eq!(format_bytes(1 << 50), "1.0 PB");
        assert_eq!(format_bytes(i64::MAX), "8192.0 PB");
    }

    #[test]
    fn uptime_uses_python_floor_division() {
        assert_eq!(format_uptime(59.9), "0m");
        assert_eq!(format_uptime(3661.0), "1h 1m");
        assert_eq!(format_uptime(141_600.5), "1d 15h 20m");
        assert_eq!(format_uptime(-5.0), "23h 59m");
        assert_eq!(py_divmod(-5.0, 86400.0), (-1.0, 86395.0));
    }

    #[test]
    fn names_are_capitalized_like_python() {
        assert_eq!(capitalize("wayland"), "Wayland");
        assert_eq!(capitalize("x11"), "X11");
        assert_eq!(capitalize("ubuntu:gnome"), "Ubuntu:gnome");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn lspci_names_match_the_original_cleanup() {
        let amd = "00:00.0 Host bridge: Advanced Micro Devices, Inc. [AMD] Device 14d8\n\
            03:00.0 VGA compatible controller: Advanced Micro Devices, Inc. [AMD/ATI] Navi 44 [Radeon RX 9050 / 9060 XT] (rev c0)\n";
        assert_eq!(parse_lspci(amd).as_deref(), Some("AMD/ATI"));
        let nvidia = "01:00.0 VGA compatible controller: NVIDIA Corporation AD102 [GeForce RTX 4090] (rev a1)";
        assert_eq!(parse_lspci(nvidia).as_deref(), Some("GeForce RTX 4090"));
        let intel = "00:02.0 VGA compatible controller: Intel Corporation Alder Lake-P GT2 [Iris Xe Graphics] (rev 0c)";
        assert_eq!(
            parse_lspci(intel).as_deref(),
            Some("Alder Lake-P GT2 [Iris Xe Graphics]")
        );
        let other = "00:0f.0 VGA compatible controller: VMware SVGA II Adapter";
        assert_eq!(
            parse_lspci(other).as_deref(),
            Some("VMware SVGA II Adapter")
        );
        assert_eq!(parse_lspci("odd ] AMD [ 3D").as_deref(), None);
        assert_eq!(
            parse_lspci("01:00.0 3D controller: AMD ]x[").as_deref(),
            Some("")
        );
        assert_eq!(parse_lspci("00:1f.3 Audio device: Intel Corporation"), None);
    }

    fn mems(entries: &[(&str, i64)]) -> HashMap<String, i64> {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v * 1024))
            .collect()
    }

    #[test]
    fn memory_matches_psutil() {
        let meminfo =
            parse_meminfo("MemTotal:  1000 kB\nMemFree: 100 kB\nMemAvailable: 286 kB\n").unwrap();
        let usage = memory_usage(&meminfo, || 4096.0, || None).unwrap();
        assert_eq!(
            usage,
            Usage {
                used: 714 * 1024,
                total: 1000 * 1024,
                percent: 71.4
            }
        );
        assert!(parse_meminfo("MemTotal:\n").is_err());
        assert!(memory_usage(&mems(&[("MemFree:", 1)]), || 4096.0, || None).is_err());
    }

    #[test]
    fn missing_available_memory_is_estimated_like_psutil() {
        let base = [("MemTotal:", 1000), ("MemFree:", 100), ("Cached:", 300)];
        let usage = memory_usage(&mems(&base), || 4096.0, || None).unwrap();
        assert_eq!(usage.used, 600 * 1024);
        let full = mems(&[
            ("MemTotal:", 1000),
            ("MemFree:", 100),
            ("MemAvailable:", 0),
            ("Active(file):", 200),
            ("Inactive(file):", 100),
            ("SReclaimable:", 50),
        ]);
        let zoneinfo = || Some("Node 0, zone DMA\n        low      10\n  high 20\n".to_owned());
        // free - low + (300 - min(150, low)) + (50 - min(25, low)), low = 40 KiB
        let usage = memory_usage(&full, || 4096.0, zoneinfo).unwrap();
        assert_eq!(usage.total - usage.used, (100 - 40 + 260 + 25) * 1024);
    }

    #[test]
    fn disk_percent_excludes_reserved_blocks() {
        let usage = disk_usage_from(1000, 300, 250);
        assert_eq!(usage.used, 700);
        assert_eq!(usage.percent, 73.7);
        assert_eq!(disk_usage_from(0, 0, 0).percent, 0.0);
    }

    #[test]
    fn commands_behave_like_subprocess_run() {
        let ok = run(&["sh", "-c", "printf 'a\\r\\nb\\rc'"], SHORT_TIMEOUT)
            .unwrap()
            .unwrap();
        assert!(ok.success);
        assert_eq!(ok.stdout, "a\nb\nc");
        let failed = run(&["sh", "-c", "exit 3"], SHORT_TIMEOUT)
            .unwrap()
            .unwrap();
        assert!(!failed.success);
        assert!(
            run(&["linexin-no-such-program"], SHORT_TIMEOUT)
                .unwrap()
                .is_none()
        );
        assert!(run(&["/etc/os-release"], SHORT_TIMEOUT).is_err());
        assert!(run(&["sh", "-c", "printf '\\377'"], SHORT_TIMEOUT).is_err());
        assert!(run(&["sh", "-c", "printf '\\377' >&2"], SHORT_TIMEOUT).is_err());
        let started = Instant::now();
        assert!(
            run(&["sleep", "5"], Duration::from_millis(200))
                .unwrap()
                .is_none()
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
