#!/usr/bin/env python3
"""Render the original z-system-information-widget.py to PNG the way
examples/harness.rs renders the native port, for side-by-side comparison.

Run inside a headless compositor (WAYLAND_DISPLAY set, DISPLAY unset)."""

import argparse
import importlib.util
import os
import time

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GLib, Graphene, Gtk  # noqa: E402

WIDGETS = "/usr/share/linexin/widgets"

parser = argparse.ArgumentParser()
parser.add_argument("--widget", default=os.path.join(WIDGETS, "z-system-information-widget.py"))
parser.add_argument("--localization", default=os.path.join(WIDGETS, "localization"))
parser.add_argument("--lang", default="en_US")
parser.add_argument("--dark", action="store_true")
parser.add_argument("--width", type=int, default=900)
parser.add_argument("--height", type=int, default=1000)
parser.add_argument("--output")
parser.add_argument("--bottom-output")
parser.add_argument("--switch-output")
args = parser.parse_args()


def load_translations(language):
    path = os.path.join(args.localization, language, "system_information_dictionary.py")
    if not os.path.exists(path):
        return {}
    spec = importlib.util.spec_from_file_location(f"dictionary_{language}", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.translations


# The Center's injected _(): active language, then en_US, then the key.
dictionaries = [load_translations(args.lang), load_translations("en_US")]


def translate(text):
    for dictionary in dictionaries:
        if text in dictionary:
            return dictionary[text]
    return text


spec = importlib.util.spec_from_file_location("system_information_widget", args.widget)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module._ = translate


def find(widget, kind):
    if isinstance(widget, kind):
        return widget
    child = widget.get_first_child()
    while child is not None:
        found = find(child, kind)
        if found is not None:
            return found
        child = child.get_next_sibling()
    return None


# Same settle time as harness.rs: the overlay scrollbar has faded out again.
SETTLE = 3.0


def iterate(seconds):
    context = GLib.MainContext.default()
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        context.iteration(False)


def render(widget, path):
    width, height = widget.get_width(), widget.get_height()
    snapshot = Gtk.Snapshot()
    Gtk.WidgetPaintable.new(widget).snapshot(snapshot, width, height)
    node = snapshot.to_node()
    renderer = widget.get_native().get_renderer()
    viewport = Graphene.Rect().init(0, 0, width, height)
    renderer.render_texture(node, viewport).save_to_png(path)
    print(f"rendered {width}x{height} to {path}")


def scheme(dark):
    return Adw.ColorScheme.FORCE_DARK if dark else Adw.ColorScheme.FORCE_LIGHT


def finish(window, widget, app):
    if args.output:
        render(window, args.output)
    if args.bottom_output:
        adjustment = find(widget, Gtk.ScrolledWindow).get_vadjustment()
        adjustment.set_value(adjustment.get_upper() - adjustment.get_page_size())
        iterate(SETTLE)
        render(window, args.bottom_output)
        adjustment.set_value(0)
    if args.switch_output:
        Adw.StyleManager.get_default().set_color_scheme(scheme(not args.dark))
        iterate(SETTLE)
        render(window, args.switch_output)
    app.quit()
    return False


def activate(app):
    Adw.StyleManager.get_default().set_color_scheme(scheme(args.dark))
    css = Gtk.CssProvider()
    css.load_from_string(".dim-label { opacity: 0.6; }")
    Gtk.StyleContext.add_provider_for_display(
        Gdk.Display.get_default(), css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
    window = Adw.ApplicationWindow(application=app)
    window.set_default_size(args.width, args.height)
    widget = module.LinexinSysInfoWidget(hide_sidebar=False, window=window)
    window.set_content(widget)
    window.present()
    started = time.monotonic()

    def poll():
        if find(widget, Adw.ActionRow) is None and time.monotonic() - started < 20:
            return True
        print(f"data shown after {time.monotonic() - started:.2f}s")
        GLib.timeout_add(500, finish, window, widget, app)
        return False

    GLib.timeout_add(100, poll)


app = Adw.Application(flags=Gio.ApplicationFlags.NON_UNIQUE)
app.connect("activate", activate)
app.run([])
