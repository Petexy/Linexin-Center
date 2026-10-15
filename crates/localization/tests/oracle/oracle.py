"""Python oracle for linexin-localization's tests; runs the original code in
original.py. Run with `python3 -B` so no bytecode lands next to dictionaries.
Strings cross the pipe hex-encoded (UTF-8) so any content survives.

  oracle.py language            original _get_system_language() (no --lang)
  oracle.py eval FILE           translations of FILE loaded like the original
  oracle.py parse < paths       per file: L|X|F|N plus pairs (see parse())
  oracle.py run LANG DIR < reqs original helpers with DIR as localization dir
  oracle.py domains DIR         domains the original registers for DIR
  oracle.py gtk LANG DIR        scene translated by the original GTK helpers
  oracle.py check ORIGINAL      verify original.py's excerpts are verbatim
"""
import ast
import importlib.util
import sys

import original


def h(text):
    return text.encode("utf-8").hex()


def unh(text):
    return bytes.fromhex(text).decode("utf-8")


def representable(translations):
    """str keys and values without lone surrogates (Rust strings)."""
    for key, value in translations.items():
        if not (isinstance(key, str) and isinstance(value, str)):
            return False
        try:
            key.encode()
            value.encode()
        except UnicodeEncodeError:
            return False
    return True


def load_file(path):
    manager = original.WidgetLocalizationManager()
    manager.widget_translations = {}
    manager._load_translation_file("oracle", "oracle", path)
    return manager.widget_translations.get("oracle", {}).get("oracle")


def is_literal(path):
    """`translations = {str: str, ...}` after an optional docstring, per ast."""
    with open(path, "rb") as handle:
        module = ast.parse(importlib.util.decode_source(handle.read()))
    body = module.body
    if body and isinstance(body[0], ast.Expr) and isinstance(body[0].value, ast.Constant) \
            and isinstance(body[0].value.value, str):
        body = body[1:]
    if len(body) != 1 or not isinstance(body[0], ast.Assign):
        return False
    assign = body[0]
    if len(assign.targets) != 1 or not isinstance(assign.targets[0], ast.Name) \
            or assign.targets[0].id != "translations" or not isinstance(assign.value, ast.Dict):
        return False
    try:
        value = ast.literal_eval(assign.value)
    except ValueError:
        return False
    return all(isinstance(k, str) and isinstance(v, str) for k, v in value.items())


def pairs(translations):
    return " ".join(f"{h(k)}:{h(v)}" for k, v in translations.items())


def parse():
    """L = literal, X = loads but not literal, F = fails to load,
    N = loads but is not representable as str pairs."""
    for line in sys.stdin:
        path = unh(line.strip())
        translations = load_file(path)
        if translations is None or not isinstance(translations, dict):
            print("F")
            continue
        if not representable(translations):
            print("N")
            continue
        try:
            literal = is_literal(path)
        except (SyntaxError, ValueError, UnicodeDecodeError):
            literal = False
        print(("L " if literal else "X ") + pairs(translations))


def install_manager(language, directory):
    original.LOCALIZATION_BASE_DIR = directory
    manager = original.WidgetLocalizationManager()
    manager.system_language = language
    manager.widget_translations = {}
    manager._load_all_translations()
    original._localization_manager = manager


def run(language, directory):
    install_manager(language, directory)
    functions = {
        "T": original._translate_with_patterns_helper,
        "M": original._translate_markup_content_static,
        "G": original._,
        "D": lambda stem, _domain: original.select_module_key(stem + ".py"),
    }
    out = []
    for line in sys.stdin.buffer:
        kind, domain, text = line.decode().rstrip("\n").split(" ")
        try:
            result = "R" + h(functions[kind](unh(text), unh(domain)))
        except RecursionError:
            result = "E"
        out.append(result)
    sys.stdout.write("\n".join(out) + "\n")


DUMPED_PROPERTIES = ("label", "title", "subtitle", "description", "placeholder-text", "heading", "body",
                     "text", "use-markup", "use-underline")


def dump(widget, depth, lines, GObject, Gtk, Adw):
    """One line per widget (depth-first): type and string/bool properties.
    Must match dump() in tests/gtk_widgets.rs."""
    fields = [widget.__gtype__.name]
    for name in DUMPED_PROPERTIES:
        pspec = widget.find_property(name)
        if pspec is None:
            continue
        value = widget.get_property(name)
        if pspec.value_type == GObject.TYPE_STRING:
            fields.append(f"{name}=" + ("-" if value is None else h(value)))
        elif pspec.value_type == GObject.TYPE_BOOLEAN:
            fields.append(f"{name}=" + ("T" if value else "F"))
    if isinstance(widget, Adw.ComboRow) and isinstance(widget.get_model(), Gtk.StringList):
        model = widget.get_model()
        items = ",".join(h(model.get_string(i)) for i in range(model.get_n_items()))
        fields.append(f"items={items} selected={widget.get_selected()}")
    lines.append("  " * depth + " ".join(fields))
    child = widget.get_first_child()
    while child:
        dump(child, depth + 1, lines, GObject, Gtk, Adw)
        child = child.get_next_sibling()


def build_scene(Gtk, Adw):
    """Must match build_scene() in tests/gtk_widgets.rs."""
    window = Gtk.Window(title="Window")
    outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
    inner = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
    window.set_child(outer)
    outer.append(inner)
    markup = Gtk.Label()
    markup.set_markup("<b>Bold</b> and <i> Hello </i>")
    for widget in (
        Gtk.Label(label="Hello"),
        Gtk.Label.new_with_mnemonic("_Open"),
        markup,
        Gtk.Label(label="Downloaded 5 files"),
        Gtk.Label(label="Downloaded Nested files"),
        Gtk.Label(label="Same"),
        Gtk.Button(label="Open"),
        Gtk.Button.new_with_mnemonic("_Cancel"),
        Gtk.ToggleButton(label="Open"),
        Gtk.CheckButton(label="Hello"),
        Gtk.Frame(label="Group"),
        Gtk.Entry(placeholder_text="Type here"),
        Gtk.SearchEntry(placeholder_text="Search…"),
        Gtk.PasswordEntry(placeholder_text="Password"),
        Adw.Banner(title="Hello"),
    ):
        inner.append(widget)
    model = Gtk.StringList.new(["First", "Second", "Third"])
    combo = Adw.ComboRow(title="Title", model=model)
    combo.set_selected(1)
    group = Adw.PreferencesGroup(title="Group", description="Description")
    for row in (
        Adw.ActionRow(title="Title", subtitle="Subtitle"),
        combo,
        Adw.SwitchRow(title="Title", subtitle="Subtitle"),
        Adw.ExpanderRow(title="Expander", subtitle="Subtitle"),
        Adw.EntryRow(title="Entry row"),
    ):
        group.add(row)
    outer.append(group)
    outer.append(Adw.PreferencesPage(title="Page"))
    status = Adw.StatusPage(title="Status", description="Description")
    status.set_child(Gtk.Label(label="Nested"))
    outer.append(status)
    alert = Adw.AlertDialog(heading="Heading", body="Body")
    alert.add_response("cancel", "Cancel")
    alert.set_extra_child(Gtk.Label(label="Nested"))
    message = Adw.MessageDialog(heading="Heading", body="Downloaded 5 files")
    message.add_response("cancel", "_Cancel")
    return window, [alert, message]


def gtk(language, directory):
    import gi
    gi.require_version("Gtk", "4.0")
    gi.require_version("Adw", "1")
    from gi.repository import Adw, GObject, Gtk
    Adw.init()
    original.Gtk, original.Adw = Gtk, Adw
    install_manager(language, directory)
    window, dialogs = build_scene(Gtk, Adw)
    original._translate_widget_recursive_static(window, "demo")
    for dialog in dialogs:
        original._translate_dialog(dialog, "demo")
    lines = []
    for widget in [window] + dialogs:
        dump(widget, 0, lines, GObject, Gtk, Adw)
    print("\n".join(lines))


def check(path):
    with open(path, encoding="utf-8") as handle:
        source = handle.read()
    with open(original.__file__, encoding="utf-8") as handle:
        vendored = handle.read()
    excerpts = [part.split("# END excerpt\n")[0] for part in vendored.split("# BEGIN excerpt\n")[1:]]
    missing = [excerpt.splitlines()[0] for excerpt in excerpts if excerpt not in source]
    if missing:
        sys.exit(f"excerpts no longer in {path}: {missing}")
    print(f"{len(excerpts)} excerpts verbatim")


def main():
    command = sys.argv[1]
    if command == "language":
        print(original.WidgetLocalizationManager()._get_system_language())
    elif command == "eval":
        translations = load_file(sys.argv[2])
        if not isinstance(translations, dict):
            sys.exit(f"{sys.argv[2]}: no translations dict")
        for key, value in translations.items():
            print(h(key), h(value))
    elif command == "parse":
        parse()
    elif command == "run":
        sys.setrecursionlimit(1000)
        run(sys.argv[2], sys.argv[3])
    elif command == "domains":
        install_manager(original.FALLBACK_LANGUAGE, sys.argv[2])
        for domain in sorted(original.get_localization_manager().widget_translations):
            print(h(domain))
    elif command == "gtk":
        gtk(sys.argv[2], sys.argv[3])
    elif command == "check":
        check(sys.argv[2])
    else:
        sys.exit(f"unknown command {command}")


if __name__ == "__main__":
    main()
