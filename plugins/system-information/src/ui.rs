//! Widget tree of the original `LinexinSysInfoWidget`.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib, pango};

use crate::info::{self, SystemInfo, Usage};

/// Translation domain: `localization/<lang>/system_information_dictionary.py`.
pub const DOMAIN: &str = "system_information";

fn tr(text: &str) -> String {
    linexin_plugin_api::tr(DOMAIN, text)
}

/// Run a GTK callback without letting a panic unwind into C.
pub fn guard(what: &str, callback: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(callback)).is_err() {
        eprintln!("System Information: {what} panicked");
    }
}

/// Widgets filled in once the worker thread has collected the data.
struct View {
    os_label: gtk::Label,
    system: adw::PreferencesGroup,
    session: adw::PreferencesGroup,
    hardware: adw::PreferencesGroup,
    storage: adw::PreferencesGroup,
}

/// Build the widget. `window` is the Center's window, resized to 1200×900 in
/// single-widget mode like the original.
pub fn build(hide_sidebar: bool, window: Option<gtk::Window>) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scrolled.set_vexpand(true);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(700);
    clamp.set_tightening_threshold(500);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_top(24);
    content.set_margin_bottom(36);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let header = gtk::Box::new(gtk::Orientation::Vertical, 8);
    header.set_halign(gtk::Align::Center);
    header.set_margin_bottom(8);

    let picture = gtk::Picture::new();
    picture.set_can_shrink(false);
    picture.set_content_fit(gtk::ContentFit::ScaleDown);
    picture.set_size_request(192, 192);
    picture.set_halign(gtk::Align::Center);
    header.append(&picture);
    apply_os_logo(&picture);
    let style_manager = adw::StyleManager::default();
    let logo_handler = style_manager.connect_dark_notify({
        let picture = picture.downgrade();
        move |_| {
            guard("logo update", || {
                if let Some(picture) = picture.upgrade() {
                    apply_os_logo(&picture);
                }
            })
        }
    });

    let os_label = gtk::Label::new(Some(""));
    os_label.add_css_class("title-1");
    os_label.set_halign(gtk::Align::Center);
    header.append(&os_label);

    if let Some(uname) = info::uname() {
        let hostname = gtk::Label::new(Some(&uname.nodename));
        hostname.add_css_class("dim-label");
        hostname.set_halign(gtk::Align::Center);
        header.append(&hostname);
    }
    content.append(&header);

    let group = |title: &str| {
        let group = adw::PreferencesGroup::new();
        group.set_title(&markup_title(tr(title)));
        content.append(&group);
        group
    };
    let view = View {
        os_label,
        system: group("System"),
        session: group("Session"),
        hardware: group("Hardware"),
        storage: group("Storage & Memory"),
    };

    clamp.set_child(Some(&content));
    scrolled.set_child(Some(&clamp));
    root.append(&scrolled);

    let loader = load_system_info(view);

    let resize = Rc::new(RefCell::new(None::<glib::SourceId>));
    if let Some(window) = window.filter(|_| hide_sidebar) {
        let window = window.downgrade();
        let pending = resize.clone();
        let source = glib::idle_add_local_once(move || {
            pending.take();
            guard("window resize", || {
                if let Some(window) = window.upgrade() {
                    window.set_default_size(1200, 900);
                }
            })
        });
        resize.replace(Some(source));
    }

    // Nothing started here may outlive the widget.
    let cleanup = RefCell::new(Some((style_manager, logo_handler)));
    root.connect_destroy(move |_| {
        guard("cleanup", || {
            loader.abort();
            if let Some(source) = resize.take() {
                source.remove();
            }
            if let Some((style_manager, handler)) = cleanup.take() {
                style_manager.disconnect(handler);
            }
        })
    });
    root
}

/// Group titles are Pango markup and the original passes them unescaped:
/// GTK rejects "Storage & Memory" (and translations with a bare `&`), logs a
/// warning and shows no title. Such a title is left empty here, which renders
/// the same without the warning.
fn markup_title(title: String) -> String {
    if pango::parse_markup(&title, '\0').is_ok() {
        title
    } else {
        String::new()
    }
}

/// Collect the data on a worker thread and show it on idle, like the original's
/// thread and `GLib.idle_add`. The task is aborted when the widget is destroyed.
fn load_system_info(view: View) -> glib::JoinHandle<()> {
    let (sender, receiver) = futures_channel::oneshot::channel();
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
    // A dropped sender (no worker) shows the error row as well.
    glib::MainContext::default().spawn_local_with_priority(
        glib::Priority::DEFAULT_IDLE,
        async move {
            let info = receiver.await.ok().flatten();
            guard("update", || update(&view, info));
        },
    )
}

fn update(view: &View, info: Option<SystemInfo>) {
    let Some(info) = info else {
        let error = tr("Failed to load system information");
        view.system
            .add(&action_row(&tr("Error"), &error, "dialog-error"));
        return;
    };
    view.os_label.set_label(&info.os_name);

    let system = &view.system;
    if let Some(version_id) = &info.version_id {
        system.add(&action_row(
            &tr("Version ID"),
            version_id,
            "application-certificate",
        ));
    }
    if let Some(version_date) = &info.version_date {
        system.add(&action_row(
            &tr("Version Date"),
            version_date,
            "preferences-system-time",
        ));
    }
    system.add(&action_row(
        &tr("Kernel"),
        &info.kernel,
        "application-x-firmware",
    ));
    system.add(&action_row(
        &tr("Uptime"),
        &info.uptime,
        "preferences-system-time",
    ));

    let session = &view.session;
    session.add(&action_row(
        &tr("Session Type"),
        &info.session_type,
        "preferences-desktop-display",
    ));
    session.add(&action_row(
        &tr("Desktop Environment"),
        &info.desktop_env,
        "preferences-desktop",
    ));
    session.add(&action_row(
        &tr("Window Manager"),
        &info.window_manager,
        "preferences-desktop-wallpaper",
    ));

    // The original appends the core count untranslated.
    let cpu = match info.cpu_count {
        Some(count) => format!("{} ({count} cores)", info.cpu_info),
        None => info.cpu_info,
    };
    view.hardware
        .add(&action_row(&tr("Processor"), &cpu, "applications-system"));
    view.hardware.add(&action_row(
        &tr("Graphics"),
        &info.gpu_info,
        "video-display",
    ));

    view.storage
        .add(&usage_row(&tr("Memory"), info.memory, "drive-harddisk"));
    view.storage
        .add(&usage_row(&tr("Disk Usage"), info.disk, "drive-harddisk"));
}

fn action_row(title: &str, subtitle: &str, icon_name: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(title);
    row.set_subtitle(subtitle);
    row.set_subtitle_selectable(true);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(20);
    row.add_prefix(&icon);
    row
}

fn usage_row(title: &str, usage: Usage, icon_name: &str) -> adw::ActionRow {
    let used = info::format_bytes(usage.used);
    let total = info::format_bytes(usage.total);
    let row = action_row(title, &format!("{used} / {total}"), icon_name);

    let bar_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar_box.set_valign(gtk::Align::Center);

    let level_bar = gtk::LevelBar::new();
    level_bar.set_min_value(0.0);
    level_bar.set_max_value(100.0);
    level_bar.set_value(usage.percent);
    level_bar.set_size_request(120, 8);
    level_bar.set_valign(gtk::Align::Center);
    bar_box.append(&level_bar);

    let percent = gtk::Label::new(Some(&format!("{:.0}%", usage.percent)));
    percent.add_css_class("caption");
    percent.add_css_class("dim-label");
    percent.set_width_chars(4);
    bar_box.append(&percent);

    row.add_suffix(&bar_box);
    row
}

/// `LOGO` from /etc/os-release, or `gnome-logo` like GNOME Settings.
pub(crate) fn logo_base_name() -> String {
    info::read_text("/etc/os-release")
        .ok()
        .and_then(|text| {
            text.lines()
                .filter_map(|line| line.strip_prefix("LOGO="))
                .map(|logo| logo.trim().trim_matches('"').trim_matches('\''))
                .find(|logo| !logo.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "gnome-logo".to_owned())
}

/// GNOME Settings' logo lookup: `-text-dark`, `-text`, `-dark`, then the base name.
fn apply_os_logo(picture: &gtk::Picture) {
    let logo = logo_base_name();
    let dark = adw::StyleManager::default().is_dark();
    let mut candidates = Vec::with_capacity(4);
    if dark {
        candidates.push(format!("{logo}-text-dark"));
    }
    candidates.push(format!("{logo}-text"));
    if dark {
        candidates.push(format!("{logo}-dark"));
    }
    candidates.push(logo);
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let names: Vec<&str> = candidates.iter().map(String::as_str).collect();
    let icon = gio::ThemedIcon::from_names(&names);
    let paintable = gtk::IconTheme::for_display(&display).lookup_by_gicon(
        &icon,
        192,
        picture.scale_factor(),
        picture.direction(),
        gtk::IconLookupFlags::empty(),
    );
    picture.set_paintable(Some(&paintable));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_gtk_cannot_parse_as_markup_stay_empty() {
        assert_eq!(markup_title("Storage & Memory".into()), "");
        assert_eq!(markup_title("Speicher & Arbeitsspeicher".into()), "");
        assert_eq!(markup_title("Pamięć i Dysk".into()), "Pamięć i Dysk");
        assert_eq!(markup_title("A &amp; B".into()), "A &amp; B");
    }
}
