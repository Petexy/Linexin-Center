# Developing Linexin Center

How this repository is built, tested and put together. What it is and how
to install and use it: [README.md](README.md).

A Rust (GTK4 + libadwaita) rewrite of Linexin Center and a drop-in replacement
for the Python package: same command, application id
(`github.petexy.linexincenter`), desktop entry, configuration file, widgets
directory, translations and look. Existing Python widgets keep working
unchanged next to native widgets compiled as shared libraries (`.so`).

## Install layout

```text
/usr/bin/linexin-center
/usr/share/applications/github.petexy.linexincenter.desktop
/usr/share/icons/github.petexy.linexincenter.svg, computer-linexin.svg
/usr/share/linexin/widgets/*.py                         Python widgets (other packages)
/usr/share/linexin/widgets/*.so                         native widgets, e.g.
/usr/share/linexin/widgets/z-system-information-widget.so
/usr/share/linexin/widgets/localization/<lang>/<domain>_dictionary.py
~/.config/linexin/linexin-center.json                   widget_order, hidden_widgets,
                                                        compact_mode, has_launched_before
```

`packaging/PKGBUILD` builds this layout (`cd packaging && makepkg -si`). The
package keeps the name `linexin-center`, so widget packages depending on it stay
installed and the Python System Information widget is replaced by its native port.

## How widgets are loaded

All `*.py` and `*.so` files of the widgets directory are widgets. When a `.so`
and a `.py` share a file stem, the `.so` wins. The sidebar order follows
`widget_order` (file names), then files by modification time; without a saved
order it is alphabetical. Names saved by the Python package (`<stem>.py`) map
onto an installed `<stem>.so`, so order and hidden state survive the upgrade.

Like the original, the sidebar is filled from metadata only. The selected widget
is created first; the others are preloaded in the background, one at a time.
`-w NAME` shows a single widget without the sidebar (`NAME` is matched like the
original: case-insensitive, `-`/space/`_` equivalent, extension optional).

On Plasma and Hyprland the window is a plain `GtkApplicationWindow` with server
decorations and the Kinexin colors; elsewhere it is an `AdwApplicationWindow`
whose window controls follow `org.gnome.desktop.wm.preferences button-layout`.

## Translations

Dictionaries are the original Python files
`<widgets dir>/localization/<lang>/<domain>_dictionary.py` containing
`translations = {"English": "Translated", ...}`, shared by the Center (domain
`widget_loader`), Python widgets and native widgets (domain = `widget_id`). The
language comes from `-l/--lang` or the locale (`LC_ALL`, `LC_CTYPE`, `LANG`),
falling back to `en_US`. Lookups use the active language, then `en_US`, then
the original's `{}` placeholder patterns. Sidebar names are translated with the
widget's own domain. Files made only of `translations = {...}`, names bound to
strings (`PROMPT = "..."`) and `translations.update({...})` are read natively
(the same result Python gives, checked by the tests against every dictionary
on the system); anything else is evaluated by the embedded Python, which the
LineXinBar front end does not start.

## Native widgets

A native widget is a `cdylib` exporting `linexin_widget_v1`, defined in
`crates/plugin-api` (C ABI version 1, versioned/size-checked structures). The
entry point receives the host services (`HostApiV1`: translation, translating a
widget tree or dialog, the command lock, and the shared sudo helper) and returns
a static descriptor:

```rust
use linexin_plugin_api as api;

static WIDGET: api::WidgetDescriptorV1 = api::WidgetDescriptorV1::new(
    c"system_information",                     // widget_id = dictionary domain
    c"System Information",                     // untranslated sidebar name
    c"/usr/share/icons/computer-linexin.svg",  // file path or themed icon name
    Some(create),                              // returns a GtkWidget (transfer full)
);
api::export_widget!(WIDGET);
// in create(): api::tr("system_information", "Kernel") for every visible string
```

Install it as `/usr/share/linexin/widgets/<name>.so` with its dictionaries under
`localization/<lang>/<widget_id>_dictionary.py`. Optional callbacks provide the
single-widget header bar controls and a close-request hook, and for a voice
widget: `--voice`/`--conversation` in `CreateContextV1`, `reactivate_voice`
(SIGUSR1), the `--compact` window (`with_compact_voice`), and on LineXinBar the
same in `LxbContextV1` with `with_lxb_reactivate_voice`. The host also answers
`sudo_askpass_path` and `window_compact_mode`. Fields appended to version one
are read only where `struct_size` covers them, so older widgets keep loading
and newer ones load in an older Center (without what it lacks). Broken libraries
(wrong version, truncated descriptor, missing symbol, ...) are reported on stderr
and shown as an error page; they never prevent other widgets from loading.
Libraries stay loaded until the Center exits. See `plugins/system-information`.

## LineXinBar

On a [LineXinBar](https://github.com/Petexy/LineXinBar) session
(`XDG_CURRENT_DESKTOP` names `LineXinBar`) with lxb-toolkit installed, the
Center does not start GTK: it is an
[lxb-toolkit](https://github.com/Petexy/lxb-toolkit) application in
LineXinBar's own style, driven by controller, keyboard and pointer, and laid
out the way LineXinBar's Software Hub (DistriBumpy) is: one glass pane over the
wallpaper, the widgets in a sidebar card at its left (each with its own icon
and translated name), the chosen widget's page beside it under its own heading
(the widget's icon and name), and along the foot the row saying what the
buttons do — drawn, not spelled out:
a picture of the button (Enter or South, Escape or East, the right mouse button
or North, following what the hands are on) beside the word for what it does,
and each pair is also clickable. On a window too narrow for both, the sidebar
and the page slide as one view. Up/Down choose, Enter or Right moves into the
page, Back or Left comes out, Back in the sidebar asks whether to leave.
"Edit widgets", pinned under a rule at the bottom of the sidebar where the GTK
window has its Edit button, is the GTK window's edit mode: its page lists every
widget, hidden ones too, each with a switch saying
whether the sidebar shows it (Enter turns it over) and Options to move it up or
down. Options (Menu, the right mouse button or North) on a widget in the
sidebar moves it, hides it, creates or removes its desktop shortcut (as root
through the session's polkit agent) and goes to "Edit widgets". All of it is
saved to the same config as the GTK window. The pointer lights what it
moves onto (the sidebar's entries and every control of a page; the list does
not scroll under it), and a click presses it. `-w` shows one widget's page
alone. Hovering needs lxb-toolkit 0.9.4 (`lxb_page_hovered`);
with 0.9.3 a click still carries the light, and "Edit widgets" says Shown or
Hidden in words where the toolkit draws no switches.

The icons are the widgets' own (their `icon`, looked up as the GTK window
does, then in the icon theme and under `$XDG_DATA_DIRS`), drawn with
`lxb_draw_picture`, which lxb-toolkit exports from 0.9.4; with 0.9.3
the widget's shell mark (`with_lxb_page`'s `mark`) stands in for
its icon. Pages draw their own pictures the same way (`linexin_lxb::Emblem`,
`icon_file`): Linexin Hello's apps, Package Manager's packages, the
installers' screenshots, System Information's system logo. Text fields
(Package Manager's search, what is asked of Alexy, its settings) take text
through `lxb_page_taking_text` and `lxb_page_typed`, from the same commit
(with 0.9.3 a field is shown but not typed into); with LineXinBar's on-screen keyboard, the field stays clear
of it (`lxb_page_board`).

Only widgets with a LineXinBar page are listed there; GTK-only widgets (all the
Python ones, e.g. the System Updater, which LineXinBar's own Settings > Updates
replaces) are not. `liblxb_app.so` is opened with `dlopen` only on such a
session (`crates/lxb`; nothing links it, neither the Center nor a widget), so
no package depends on lxb-toolkit (the Center's PKGBUILD lists it in `optdepends`).
Without it, with one too old (lacking a function the front end cannot do
without), or when its window cannot open (no display, no graphics adapter),
the Center says why on stderr and opens the GTK window instead, with every
widget, as on any other desktop: nothing of the Center's LineXinBar front end
(its widgets, catalog, signal handlers) is set up before its window has opened
(`crates/center/tests/lxb_fallback.rs` checks each case against a fake
`liblxb_app.so`). `-w` with a widget that has no LineXinBar page (Desktop
Presets, a Python widget) opens it in the GTK window too.
`LINEXIN_FRONTEND=lxb|gtk` overrides the choice, and
`LINEXIN_LXB_SHOT=out.png[,WIDTHxHEIGHT][,page|menu|edit][,wait=SECONDS]` draws one
frame to a PNG without a display; with `,frames=N` the Nth, after the actions
named with `,act=FRAME:ACTION` (`accept`, `down`, `menu`, ...).

A widget gets its LineXinBar page with three more callbacks
(`WidgetDescriptorV1::with_lxb_page(mark, lxb_create, lxb_page, lxb_destroy)`):
the Center hands `lxb_page` an `LxbFrameV1` each frame (the `lxb_page*`, the
page's rectangle, whether it has the light, and that frame's actions) and the
widget answers `LXB_PAGE_LEAVE` to give the light back. `crates/lxb` is the
binding to draw with (`Flow` lays out a page of rows, two-line entries,
pairs, switches, text fields, galleries, buttons and grids of cards that
scrolls and follows the light, Up/Down and Left/Right walking it as the
controls stand; `SidePanel` puts a second flow beside the page in a wide
window, as the GTK pages' sidebars, the page's margin between the two; cards and pictures are lit in their own
shape with `lxb_page_light_shaped` (lxb-toolkit 0.9.4); a page's first `head` is its heading, its icon before its name,
pinned at the top while the rest scrolls beneath it; no text runs past its
room: names and headings wrap (a page's name onto two lines), a word wider
than a line is broken, and labels of rows and buttons end in an ellipsis; `LINEXIN_LXB_TYPED`
scripts what is typed for pictures taken without a keyboard);
`plugins/demo` is the smallest example and
`plugins/system-information` a real one. Widgets built before these fields
existed keep loading (and are not listed on LineXinBar).

## Python widgets

Python widgets run in an embedded CPython (`crates/python-bridge`); PyGObject
wraps the same GTK objects as the Rust host. Modules get the original injected
globals (`_`, `sudo_manager`, `translate_dialog`, `translate_window`), the
window keeps the attributes widgets use (`hide_sidebar`, `active_widget_id`,
`refresh_widgets()`, `resize_window()`, ...), and privileged subprocesses lock
the UI as before. If Python cannot start, native widgets still work.

## Command line

```text
linexin-center [-w NAME] [--voice] [--compact] [--conversation ID] [-l CODE] [-v]
```

`--compact` with `-w` opens the widget's compact voice window; `SIGUSR1`
restarts voice input. On LineXinBar there is no compact window: `--voice` or
`--compact` with `-w` shows the widget's page alone, listening at once, and
`SIGUSR1` makes it listen again. Development only: `--widgets-dir PATH` (repeatable; replaces
the default directory, the first directory wins for equal file names), `--list`
(print the widgets in sidebar order without GTK) and `-h`.

## Development

Requirements: Rust 1.88+ (edition 2024), GTK >= 4.12 and libadwaita >= 1.5 with
development files, `pkg-config`, Python 3 with development files and PyGObject.

```sh
./build.sh                          # debug build, staged as build/usr/...
./run.sh                            # staged widgets only
./run.sh --with-system-widgets      # plus /usr/share/linexin/widgets
./run.sh --with-system-widgets -w a-system-updater -l pl_PL
```

`build.sh` stages `build/usr/bin/linexin-center` next to
`build/usr/share/linexin/widgets` (System Information `.so` and dictionaries),
which that binary uses instead of `/usr/share/linexin/widgets` (the original
"running from source" rule). `run.sh` starts a separate instance
(`LINEXIN_NEW_INSTANCE=1`), so a running Linexin Center is not activated instead.

The panel icon comes from the desktop entry matching the application id. With
the Linexin Center package installed it is already there. Otherwise
`./run.sh --install-desktop-entry [DATA_DIR]` writes an entry and icon for the
staged build into `DATA_DIR` (default `build/xdg-data`; the desktop only reads
its own data directories, e.g. `~/.local/share`, so pass that explicitly if you
want it) and `--remove-desktop-entry [DATA_DIR]` removes them. Nothing is
installed anywhere unless you ask for it.

### Tests

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 tests/native_failures.py    # loader: rejected descriptors, dedupe, ordering
```

`tests/native_failures.py` compiles the fixtures in `tests/fixtures/native` with
plain `rustc` and runs `linexin-center --list` on temporary directories.

The localization tests check the Rust loader against the original Python one
(with `python3`) over this checkout's `data/localization` and, where Linexin
Center is installed, `/usr/share/linexin/widgets/localization`. To compare
the original projects' dictionaries too, name their checkouts (or a folder
holding them) in `LINEXIN_ORIGINAL_SOURCES`, `:`-separated;
`LINEXIN_ORIGINAL_CENTER` names the original `linexin-center` script when it
is not in one of them. A name that does not exist fails the tests.

`crates/center/tests/gui.rs` drives the real window with native fixture widgets
(normal, compact, edit and single-widget modes, auto-compact, legacy config) and
can render PNGs. It is skipped unless `LINEXIN_GUI_TESTS=1`; run it inside a
private compositor, never on your desktop:

```sh
dbus-run-session -- kwin_wayland --virtual --no-lockscreen --socket linexin-test &
LINEXIN_GUI_TESTS=1 WAYLAND_DISPLAY=linexin-test XDG_CURRENT_DESKTOP=GNOME \
  GSETTINGS_BACKEND=memory ADW_DISABLE_PORTAL=1 \
  LINEXIN_GUI_SNAPSHOTS=target/gui-shots cargo test -p linexin-center --test gui
```

Use `XDG_CURRENT_DESKTOP=KDE` for the Plasma variant and `LINEXIN_GUI_LAYOUT`
to try another button layout.

## Repository layout

```text
crates/center/              linexin-center: application, window, widget loading
crates/plugin-api/          native widget C ABI (v1)
crates/localization/        *_dictionary.py loader and translation helpers
crates/services/            command lock and shared sudo helper
crates/python-bridge/       embedded CPython for legacy widgets
crates/lxb/                 lxb-toolkit binding (dlopen) for the LineXinBar front end
plugins/system-information/ native System Information widget
plugins/demo/               example native widget
data/                       dictionaries (widget_loader, system_information) and icons
packaging/                  PKGBUILD and desktop entry
tests/                      native loader fixtures and tests
```
