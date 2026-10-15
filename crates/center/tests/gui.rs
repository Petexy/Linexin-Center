//! Drives the real Linexin Center window with native fixture widgets and
//! renders it to PNG. Needs a compositor; never run it on a real desktop:
//!
//!   kwin_wayland --virtual --no-lockscreen --width 1600 --height 1000 --socket linexin-test &
//!   LINEXIN_GUI_TESTS=1 WAYLAND_DISPLAY=linexin-test XDG_CURRENT_DESKTOP=GNOME \
//!     GSETTINGS_BACKEND=memory cargo test -p linexin-center --test gui
//!
//! `XDG_CURRENT_DESKTOP=KDE` checks the Plasma/Hyprland variant. Optional:
//! LINEXIN_GUI_SNAPSHOTS=<dir> (PNGs + geometry JSON), LINEXIN_GUI_LAYOUT=<button
//! layout>, LINEXIN_GUI_LANG=<language>. Without LINEXIN_GUI_TESTS the test is
//! skipped. Everything it writes stays in cargo's temporary folder for tests.

use adw::prelude::*;
use gtk::{gio, glib, graphene};
use linexin_center::config::{self, ConfigFile};
use linexin_center::paths::{self, Paths};
use linexin_center::window::{CenterWindow, Part, Services, WindowOptions};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

const FIXTURES: [(&str, &str, &str, &str); 3] = [
    (
        "a-fixture-alpha",
        "fixture_alpha",
        "Fixture Alpha",
        "/usr/share/icons/computer-linexin.svg",
    ),
    (
        "b-fixture-beta",
        "fixture_beta",
        "Fixture Beta",
        "/usr/share/icons/linexin-hello.png",
    ),
    ("c-fixture-gamma", "fixture_gamma", "Fixture Gamma", ""),
];

fn main() {
    if std::env::var_os("LINEXIN_GUI_TESTS").is_none() {
        println!("gui: skipped (set LINEXIN_GUI_TESTS=1 and run inside a test compositor)");
        return;
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.join("../..");
    let root =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("gui-test-{}", std::process::id()));
    let widgets = root.join("widgets");
    std::fs::create_dir_all(&widgets).expect("test directory");
    build_fixtures(&workspace, &widgets);
    copy_dir(
        &workspace.join("data/localization"),
        &widgets.join("localization"),
    );
    let close_log = root.join("close.log");
    // SAFETY: single-threaded; GTK is not initialized yet.
    unsafe { std::env::set_var("LINEXIN_FIXTURE_CLOSE_LOG", &close_log) };
    let snapshots = std::env::var_os("LINEXIN_GUI_SNAPSHOTS").map(PathBuf::from);
    if let Some(dir) = &snapshots {
        std::fs::create_dir_all(dir).expect("snapshot directory");
    }

    adw::init().expect("GTK needs WAYLAND_DISPLAY of a test compositor");
    if let Ok(layout) = std::env::var("LINEXIN_GUI_LAYOUT") {
        gio::Settings::new("org.gnome.desktop.wm.preferences")
            .set_string("button-layout", &layout)
            .expect("button-layout");
    }
    let language = std::env::var("LINEXIN_GUI_LANG").unwrap_or_else(|_| "en_US".into());
    linexin_localization::install(linexin_localization::Localization::load(
        &[widgets.join("localization")],
        &language,
        None,
    ));
    check_translate_widget_keeps_floating();
    // The real id: GTK derives the window icon (decoration layout "icon") from it.
    let application = adw::Application::new(Some(paths::APP_ID), gio::ApplicationFlags::NON_UNIQUE);
    application
        .register(None::<&gio::Cancellable>)
        .expect("register");

    let kinexin = paths::detect_plasma_or_hyprland();
    let config = ConfigFile::at(root.join("config/linexin/linexin-center.json"));
    let services = Rc::new(Services {
        paths: Paths {
            widgets_dirs: vec![widgets.clone()],
            share_dir: root.join("share"),
        },
        config: config.clone(),
        python: None,
        kinexin,
    });
    let prefix = if kinexin { "rust-kde" } else { "rust-gnome" };
    let mut report = Report::new(snapshots, prefix);

    // Sidebar mode.
    let center = CenterWindow::new(&application, services.clone(), WindowOptions::default());
    let window = center.window().expect("window");
    window.present();
    wait(2500);
    check_normal(&center, &window, kinexin);
    assert!(config::flag(&config.load(), config::HAS_LAUNCHED_BEFORE));
    report.capture(&center, "normal");

    center.set_compact_mode(true, false);
    wait(800);
    check_compact(&center);
    report.capture(&center, "compact");

    center.set_compact_mode(false, false);
    wait(500);
    center.toggle_edit_mode();
    wait(1500);
    assert!(center.is_editing());
    report.capture(&center, "edit");
    center.toggle_edit_mode();
    wait(1500);
    report.capture(&center, "after-edit");
    let saved = config::names(&config.load(), config::WIDGET_ORDER);
    assert_eq!(
        saved,
        [
            "a-fixture-alpha.so",
            "b-fixture-beta.so",
            "c-fixture-gamma.so"
        ]
    );

    center.select_index(2);
    wait(600);
    assert_eq!(
        center.visible_page().as_deref(),
        Some("widget_c_fixture_gamma")
    );
    center.set_compact_mode(true, true);
    wait(800);
    assert!(center.is_compact());
    report.capture(&center, "compact-animated");
    center.set_compact_mode(false, true);
    wait(800);

    // Narrow windows compact automatically; edit mode expands the sidebar and
    // re-compacts it afterwards; a wide window restores the normal sidebar.
    window.set_default_size(900, 730);
    wait(800);
    assert!(center.is_compact(), "auto-compact below 1000 px");
    center.toggle_edit_mode();
    wait(1000);
    assert!(!center.is_compact());
    center.toggle_edit_mode();
    wait(1500);
    assert!(center.is_compact(), "auto-compact restored after edit mode");
    window.set_default_size(1280, 730);
    wait(800);
    assert!(!center.is_compact());

    window.close();
    wait(200);
    let closed = std::fs::read_to_string(&close_log).unwrap_or_default();
    for (_, id, _, _) in FIXTURES {
        assert!(
            closed.lines().any(|line| line == id),
            "close_request not forwarded to {id}"
        );
    }

    // Order and hidden state saved by the Python package use `.py` names.
    config.update(|saved| {
        let order = [
            "c-fixture-gamma.py".to_owned(),
            "a-fixture-alpha.py".to_owned(),
        ];
        config::set_names(saved, config::WIDGET_ORDER, &order);
        config::set_names(
            saved,
            config::HIDDEN_WIDGETS,
            &["b-fixture-beta.py".to_owned()],
        );
    });
    let legacy = CenterWindow::new(&application, services.clone(), WindowOptions::default());
    legacy.window().expect("window").present();
    wait(1200);
    assert_eq!(
        legacy.sidebar_titles(),
        ["Fixture Gamma", "Fixture Alpha", "Fixture Beta"]
    );
    assert!(
        !legacy
            .part(Part::Row(2))
            .is_some_and(|row| row.is_visible())
    );
    assert_eq!(
        legacy.visible_page().as_deref(),
        Some("widget_c_fixture_gamma")
    );
    legacy.window().expect("window").close();
    wait(100);

    // Single-widget mode (`-w b-fixture-beta`).
    let single = CenterWindow::new(
        &application,
        services.clone(),
        WindowOptions {
            target_widget: Some("b-fixture-beta".into()),
            ..WindowOptions::default()
        },
    );
    let single_window = single.window().expect("window");
    single_window.present();
    wait(1500);
    assert_eq!(
        single.visible_page().as_deref(),
        Some("widget_b_fixture_beta")
    );
    assert_eq!(single_window.title().as_deref(), Some("Fixture Beta"));
    let header = single.part(Part::ContentHeader).expect("header");
    assert!(
        find_button(&header, "Fixture action"),
        "header bar widget of the native widget missing"
    );
    report.capture(&single, "single");
    single_window.close();

    // Unknown target: the original's error label.
    let missing = CenterWindow::new(
        &application,
        services,
        WindowOptions {
            target_widget: Some("does-not-exist".into()),
            ..WindowOptions::default()
        },
    );
    missing.window().expect("window").present();
    wait(500);
    assert_eq!(missing.visible_page().as_deref(), Some("error"));
    missing.window().expect("window").close();
    wait(100);

    report.finish();
    let _ = std::fs::remove_dir_all(&root);
    println!("gui: all checks passed ({prefix})");
}

fn check_normal(center: &CenterWindow, window: &gtk::ApplicationWindow, kinexin: bool) {
    assert_eq!(
        center.sidebar_titles(),
        ["Fixture Alpha", "Fixture Beta", "Fixture Gamma"]
    );
    assert_eq!(
        center.visible_page().as_deref(),
        Some("widget_a_fixture_alpha")
    );
    assert_eq!(window.title().as_deref(), Some("Linexin Center"));
    assert_eq!(window.is::<adw::ApplicationWindow>(), !kinexin);
    for index in 0..3 {
        let row = center
            .part(Part::Row(index))
            .and_downcast::<adw::ActionRow>()
            .expect("row");
        assert!(
            row.subtitle().is_none_or(|s| s.is_empty()),
            "rows have no subtitles"
        );
    }
    // The content header's title is an empty label (no widget title bar).
    let header = center
        .part(Part::ContentHeader)
        .and_downcast::<gtk::HeaderBar>()
        .expect("content header");
    let title = header
        .title_widget()
        .and_downcast::<gtk::Label>()
        .expect("label title");
    assert_eq!(title.text(), "");
    let sidebar_title = center
        .part(Part::SidebarTitle)
        .and_downcast::<gtk::Label>()
        .expect("sidebar title");
    assert_eq!(sidebar_title.is_visible(), !kinexin);
}

fn check_compact(center: &CenterWindow) {
    assert!(center.is_compact());
    let root = center.part(Part::Root).expect("root");
    let visible = |part| center.part(part).is_some_and(|w| w.is_visible());
    assert!(!visible(Part::ListContainer));
    assert!(visible(Part::CompactScroll));
    // The sidebar header stays above the icon strip; the bottom bar stays horizontal.
    let header = bounds(center.part(Part::SidebarHeader), &root).expect("header");
    let first_icon = bounds(center.part(Part::CompactIcon(0)), &root).expect("icon");
    assert!(first_icon.y() > header.y() + header.height());
    let bottom = center
        .part(Part::BottomBox)
        .and_downcast::<gtk::Box>()
        .expect("bottom box");
    assert_eq!(bottom.orientation(), gtk::Orientation::Horizontal);
}

/// HostApiV1::translate_widget must not sink (and then free) a widget that a
/// C plugin has created but not parented yet.
fn check_translate_widget_keeps_floating() {
    use gtk::glib::translate::ToGlibPtr;
    let label = gtk::Label::new(Some("Some text"));
    // SAFETY: plain GObject calls on a live label, mimicking a C plugin's
    // freshly created floating widget.
    unsafe {
        let pointer: *mut gtk::ffi::GtkWidget = label.upcast_ref::<gtk::Widget>().to_glib_none().0;
        gtk::glib::gobject_ffi::g_object_force_floating(pointer.cast());
        (linexin_center::host_api::HOST_API.translate_widget)(
            c"widget_loader".as_ptr(),
            pointer.cast(),
        );
        assert_ne!(
            gtk::glib::gobject_ffi::g_object_is_floating(pointer.cast()),
            0
        );
        // Turn the floating reference back into the one `label` owns.
        gtk::glib::gobject_ffi::g_object_ref_sink(pointer.cast());
    }
    assert_eq!(label.label(), "Some text");
}

fn find_button(widget: &gtk::Widget, label: &str) -> bool {
    if widget
        .downcast_ref::<gtk::Button>()
        .is_some_and(|button| button.label().as_deref() == Some(label))
    {
        return true;
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if find_button(&current, label) {
            return true;
        }
        child = current.next_sibling();
    }
    false
}

fn wait(ms: u64) {
    let main_loop = glib::MainLoop::new(None, false);
    let quit = main_loop.clone();
    glib::timeout_add_local_once(Duration::from_millis(ms), move || quit.quit());
    main_loop.run();
}

fn bounds(widget: Option<gtk::Widget>, root: &gtk::Widget) -> Option<graphene::Rect> {
    widget.filter(|w| w.is_mapped())?.compute_bounds(root)
}

fn rect_json(rect: Option<graphene::Rect>) -> Value {
    let round = |v: f32| (f64::from(v) * 10.0).round() / 10.0;
    rect.map_or(Value::Null, |r| {
        json!([
            round(r.x()),
            round(r.y()),
            round(r.width()),
            round(r.height())
        ])
    })
}

struct Report {
    dir: Option<PathBuf>,
    prefix: &'static str,
    geometry: serde_json::Map<String, Value>,
}

impl Report {
    fn new(dir: Option<PathBuf>, prefix: &'static str) -> Self {
        Self {
            dir,
            prefix,
            geometry: serde_json::Map::new(),
        }
    }

    /// Geometry in the format of the original-app harness, plus a PNG.
    fn capture(&mut self, center: &CenterWindow, state: &str) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let root = center.part(Part::Root).expect("root");
        let part = |part| rect_json(bounds(center.part(part), &root));
        let many = |make: fn(usize) -> Part| -> Value {
            (0..FIXTURES.len())
                .map(|i| rect_json(bounds(center.part(make(i)), &root)))
                .collect()
        };
        let window = center.window().expect("window");
        self.geometry.insert(
            state.into(),
            json!({
                "root": [root.width(), root.height()],
                "sidebar_container": part(Part::SidebarContainer),
                "sidebar_header": part(Part::SidebarHeader),
                "sidebar_title": part(Part::SidebarTitle),
                "list_container": part(Part::ListContainer),
                "rows": many(Part::Row),
                "row_titles": center.sidebar_titles(),
                "compact_scroll": part(Part::CompactScroll),
                "compact_buttons": many(Part::CompactIcon),
                "bottom_box": part(Part::BottomBox),
                "edit_btn": part(Part::EditButton),
                "compact_btn": part(Part::CompactButton),
                "content_header": part(Part::ContentHeader),
                "content_display": part(Part::ContentDisplay),
                "title": window.title().map(String::from),
            }),
        );
        render(&root, &dir.join(format!("{}-{state}.png", self.prefix)));
    }

    fn finish(self) {
        if let Some(dir) = self.dir {
            let text = serde_json::to_string_pretty(&Value::Object(self.geometry)).expect("json");
            std::fs::write(dir.join(format!("{}-geometry.json", self.prefix)), text)
                .expect("geometry");
        }
    }
}

fn render(widget: &gtk::Widget, path: &Path) {
    let (width, height) = (widget.width(), widget.height());
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(
        snapshot.upcast_ref::<gtk::gdk::Snapshot>(),
        f64::from(width),
        f64::from(height),
    );
    let node = snapshot.to_node().expect("render node");
    let renderer = widget
        .native()
        .and_then(|n| n.renderer())
        .expect("renderer");
    let viewport = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
    let texture = renderer.render_texture(&node, Some(&viewport));
    texture.save_to_png(path).expect("save PNG");
    println!("rendered {}", path.display());
}

fn build_fixtures(workspace: &Path, widgets: &Path) {
    let source = workspace.join("tests/fixtures/native/label_widget.rs");
    for (file, id, name, icon) in FIXTURES {
        let status = Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-type=cdylib",
                "-l",
                "gtk-4",
                "-l",
                "gobject-2.0",
            ])
            .arg(&source)
            .arg("-o")
            .arg(widgets.join(format!("{file}.so")))
            .env("FIXTURE_ID", id)
            .env("FIXTURE_NAME", name)
            .env("FIXTURE_ICON", icon)
            .env("FIXTURE_TEXT", format!("{name} content"))
            .status()
            .expect("rustc");
        assert!(status.success(), "building fixture {file} failed");
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("copy");
    for entry in std::fs::read_dir(from).expect("read data dir").flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy");
        }
    }
}
