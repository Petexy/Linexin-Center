//! The LineXinBar page: the GTK page's information in LineXinBar's own
//! language. No GTK here — it is not initialized on a LineXinBar session.

use crate::info::{self, SystemInfo, Usage};
use linexin_lxb::{Emblem, Flow, FlowFrame, Role, WidgetFrame, guard, icon_file, page_result};
use linexin_plugin_api::{self as api, LXB_PAGE_STAY, LxbContextV1, LxbFrameV1};
use std::ffi::c_void;
use std::panic::catch_unwind;
use std::path::PathBuf;
use std::ptr;
use std::sync::mpsc::{self, Receiver, TryRecvError};

const MARK: &str = "setting-info";
/// The widget's own icon, where the system names no logo of its own.
const ICON: &str = "/usr/share/icons/computer-linexin.svg";

fn tr(text: &str) -> String {
    api::tr(crate::ui::DOMAIN, text)
}

struct State {
    flow: Flow,
    hostname: Option<String>,
    /// Until the worker has answered.
    loading: Option<Receiver<Option<SystemInfo>>>,
    /// `None` once loaded means the original's error row.
    info: Option<SystemInfo>,
    /// The system's logo (the GTK page's picture), or the widget's icon.
    logo: Option<PathBuf>,
}

pub unsafe extern "C" fn create(context: *const LxbContextV1) -> *mut c_void {
    guard(ptr::null_mut(), || {
        // SAFETY: the pointer the host passed to `lxb_create`.
        if unsafe { LxbContextV1::read(context) }.is_none() {
            return ptr::null_mut();
        }
        // Collected once on a thread of its own, as the GTK page does. The
        // thread holds nothing of the page, so the page may go first.
        let (sender, receiver) = mpsc::channel();
        let unknown = tr("Unknown");
        let worker = std::thread::Builder::new()
            .name("system-information".into())
            .spawn(move || {
                let info = catch_unwind(|| info::collect(&unknown)).unwrap_or_else(|_| {
                    eprintln!("Error loading system info: collection panicked");
                    None
                });
                let _ = sender.send(info);
            });
        if let Err(error) = worker {
            eprintln!("Error loading system info: {error}");
        }
        let state = State {
            flow: Flow::new(),
            hostname: info::uname().map(|uname| uname.nodename),
            loading: Some(receiver),
            info: None,
            logo: logo(),
        };
        Box::into_raw(Box::new(state)).cast()
    })
}

pub unsafe extern "C" fn page(state: *mut c_void, frame: *const LxbFrameV1) -> u32 {
    guard(LXB_PAGE_STAY, || {
        // SAFETY: the state from `create`, alive for the call.
        let Some(state) = (unsafe { state.cast::<State>().as_mut() }) else {
            return LXB_PAGE_STAY;
        };
        // SAFETY: the host's frame, read for this call only.
        let Some(frame) = (unsafe { WidgetFrame::read(frame) }) else {
            return LXB_PAGE_STAY;
        };
        state.draw(&frame)
    })
}

pub unsafe extern "C" fn destroy(state: *mut c_void) {
    guard((), || {
        if !state.is_null() {
            // SAFETY: the state from `create`, released once.
            drop(unsafe { Box::from_raw(state.cast::<State>()) });
        }
    })
}

impl State {
    fn draw(&mut self, frame: &WidgetFrame) -> u32 {
        let page = &frame.page;
        if let Some(receiver) = &self.loading {
            match receiver.try_recv() {
                Ok(info) => {
                    self.info = info;
                    self.loading = None;
                }
                // Ask for the frame that shows the answer.
                Err(TryRecvError::Empty) => page.redraw_within(0.1),
                Err(TryRecvError::Disconnected) => self.loading = None,
            }
        }
        let input = if frame.focused {
            self.flow.input(page, &frame.actions)
        } else {
            Default::default()
        };
        let emblem = Emblem::with_picture(self.logo.as_deref(), MARK);
        let mut flow = self.flow.begin(page, frame.rect, frame.focused);
        match (&self.loading, &self.info) {
            (Some(_), _) => {
                flow.head(emblem, &tr("System Information"));
                flow.note(&tr("Loading..."));
            }
            (None, None) => {
                flow.head(emblem, &tr("System Information"));
                flow.pair(&tr("Error"), &tr("Failed to load system information"));
            }
            (None, Some(info)) => sections(&mut flow, emblem, info, self.hostname.as_deref()),
        }
        flow.finish();
        page_result(input.leave.is_some())
    }
}

/// The system's logo as the GTK page finds it (its `-dark` form first: the
/// page stands on dark glass; not the `-text` forms, whose name the heading
/// already writes), or the widget's own icon.
fn logo() -> Option<PathBuf> {
    let logo = crate::ui::logo_base_name();
    [format!("{logo}-dark"), logo]
        .iter()
        .find_map(|name| icon_file(name))
        .or_else(|| icon_file(ICON))
}

/// The GTK page's header and four groups, in its order and words.
fn sections(flow: &mut FlowFrame, emblem: Emblem, info: &SystemInfo, hostname: Option<&str>) {
    flow.head(emblem, &info.os_name);
    if let Some(hostname) = hostname {
        flow.note(hostname);
    }
    let names: Vec<String> = [
        "Version ID",
        "Version Date",
        "Kernel",
        "Uptime",
        "Session Type",
        "Desktop Environment",
        "Window Manager",
        "Processor",
        "Graphics",
        "Memory",
        "Disk Usage",
    ]
    .iter()
    .map(|name| tr(name))
    .collect();
    flow.pairs_named(&names.iter().map(String::as_str).collect::<Vec<_>>());

    flow.gap();
    flow.heading(&tr("System"));
    if let Some(version_id) = &info.version_id {
        flow.pair(&tr("Version ID"), version_id);
    }
    if let Some(version_date) = &info.version_date {
        flow.pair(&tr("Version Date"), version_date);
    }
    flow.pair(&tr("Kernel"), &info.kernel);
    flow.pair(&tr("Uptime"), &info.uptime);

    flow.gap();
    flow.heading(&tr("Session"));
    flow.pair(&tr("Session Type"), &info.session_type);
    flow.pair(&tr("Desktop Environment"), &info.desktop_env);
    flow.pair(&tr("Window Manager"), &info.window_manager);

    flow.gap();
    flow.heading(&tr("Hardware"));
    // The original appends the core count untranslated.
    let cpu = match info.cpu_count {
        Some(count) => format!("{} ({count} cores)", info.cpu_info),
        None => info.cpu_info.clone(),
    };
    flow.pair(&tr("Processor"), &cpu);
    flow.pair(&tr("Graphics"), &info.gpu_info);

    flow.gap();
    flow.heading(&tr("Storage & Memory"));
    flow.pair(&tr("Memory"), &usage(info.memory));
    meter(flow, info.memory.percent);
    flow.pair(&tr("Disk Usage"), &usage(info.disk));
    meter(flow, info.disk.percent);
}

/// The GTK usage row's bar, under its line: the track, and the share used.
fn meter(flow: &mut FlowFrame, percent: f64) {
    let (height, air, widest) = {
        let page = flow.page();
        (
            page.scaled(6.0).max(2.0),
            page.scaled(4.0),
            page.scaled(520.0),
        )
    };
    flow.space(air);
    if let Some([x, y, width, height]) = flow.space(height) {
        let width = width.min(widest);
        let page = flow.page();
        page.draw_rule([x, y, width, height], Role::GlassRaised);
        let filled = width * (percent / 100.0).clamp(0.0, 1.0) as f32;
        if filled > 0.0 {
            page.draw_rule([x, y, filled, height], Role::Accent);
        }
    }
    flow.gap();
}

/// The GTK usage row's subtitle and percentage on one line.
fn usage(usage: Usage) -> String {
    format!(
        "{} / {} · {:.0}%",
        info::format_bytes(usage.used),
        info::format_bytes(usage.total),
        usage.percent
    )
}
