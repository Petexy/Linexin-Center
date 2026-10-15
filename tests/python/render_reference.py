"""Reference render of a legacy widget under plain PyGObject, the way the
original Python Center hosted it (module executed, then `_`, sudo_manager and
the translate helpers injected), for pixel comparison with the Rust host.

Run inside a private compositor only (see
crates/python-bridge/examples/run_bridge_check.sh), e.g.:
    python3 -B tests/python/render_reference.py WIDGET.py OUT_DIR [LANG]
"""
import importlib.util
import os
import sys
import time

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, GLib, Graphene, Gtk  # noqa: E402

path, out_dir = sys.argv[1], sys.argv[2]
language = sys.argv[3] if len(sys.argv) > 3 else "en_US"
stem = os.path.splitext(os.path.basename(path))[0]
localization = os.path.join(os.path.dirname(path), "localization")


def select_key():
    """The original _prepare_module dictionary key selection."""
    known = {name[:-len("_dictionary.py")]
             for language_dir in (os.listdir(localization) if os.path.isdir(localization) else [])
             for name in os.listdir(os.path.join(localization, language_dir))
             if name.endswith("_dictionary.py")}
    keys = [stem] + ([stem[2:]] if len(stem) > 2 and stem[1] == "-" else [])
    keys += [key.replace("-", "_") for key in keys]
    for key in keys:
        if key in known:
            return key
    for key in keys:
        clean = key.replace("_widget", "").replace("-widget", "")
        if clean in known:
            return clean
    return keys[0]


domain = select_key()


def load_translations():
    dictionary = os.path.join(localization, language, f"{domain}_dictionary.py")
    if not os.path.exists(dictionary):
        return {}
    spec = importlib.util.spec_from_file_location(f"{domain}_{language}_dict", dictionary)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.translations


translations = load_translations()
spec = importlib.util.spec_from_file_location(stem, path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module._ = lambda message, widget_id=domain: translations.get(message, message) if message else ""
module.translate_dialog = module.translate_window = lambda widget: None
module.sudo_manager = None

app = Adw.Application(application_id="io.github.linexin.RenderReference")


def render(widget, name):
    paintable = Gtk.WidgetPaintable.new(widget)
    snapshot = Gtk.Snapshot()
    width, height = widget.get_width(), widget.get_height()
    paintable.snapshot(snapshot, width, height)
    node = snapshot.to_node()
    viewport = Graphene.Rect().init(0, 0, width, height)
    texture = widget.get_native().get_renderer().render_texture(node, viewport)
    file = os.path.join(out_dir, f"reference-{name}-{language}.png")
    texture.save_to_png(file)
    print(f"RENDERED {file} ({width}x{height})")


def activate(app):
    window = Adw.ApplicationWindow(application=app, title="Linexin Center")
    window.set_default_size(1280, 730)
    classes = [value for value in vars(module).values()
               if isinstance(value, type) and issubclass(value, Gtk.Widget)
               and value.__module__ == stem and not issubclass(value, Gtk.Window)]
    widget = next(cls(hide_sidebar=False, window=window) for cls in classes
                  if "widgetname" in cls.__init__.__code__.co_names)
    window.set_content(widget)
    window.present()
    deadline = time.monotonic() + 4
    context = GLib.MainContext.default()
    while time.monotonic() < deadline:
        context.iteration(False)
        time.sleep(0.005)
    render(widget, "widget")
    render(window, "window")
    app.quit()


app.connect("activate", activate)
app.run([])
