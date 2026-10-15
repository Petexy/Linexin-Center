"""Python side of the Linexin Center widget bridge.

Embedded by linexin-python-bridge. Ports the original Center's widget module
contract (metadata scan, import, injected helpers, constructor fallbacks),
its subprocess lock patches and the SudoManager API widgets use, on top of
the host services in _linexin_host (Rust).
"""
import ast
import importlib.util
import os
import subprocess
import threading
import traceback

import _linexin_host as _host

# Objects the GTK side references only through raw pointers stay alive here.
_WINDOWS = {}
_INSTANCES = []


# --- Subprocess patches (original lines 247-251 and 438-534) ---------------

_original_popen = subprocess.Popen
_original_run = subprocess.run
_original_call = subprocess.call
_original_check_call = subprocess.check_call
_original_check_output = subprocess.check_output


def _should_lock_command(args, kwargs):
    """Only privileged commands (those using the sudo wrapper) lock the UI."""
    cmd = args[0] if args else kwargs.get('args')
    if cmd is None:
        return False
    if isinstance(cmd, (list, tuple)) and len(cmd) > 0:
        exe = str(cmd[0])
    elif isinstance(cmd, str):
        exe = cmd.split()[0] if cmd.strip() else ''
    else:
        return False
    if _host.sudo_is_wrapper_command(exe):
        return True
    # shell=True: the wrapper may appear anywhere in the command string.
    return isinstance(cmd, str) and _host.sudo_is_wrapper_command(cmd)


class _LockedPopen(_original_popen):
    """subprocess.Popen that locks the UI while a privileged command runs."""

    def __init__(self, *args, **kwargs):
        if _should_lock_command(args, kwargs):
            _host.lock()
            try:
                super().__init__(*args, **kwargs)
            except BaseException:
                _host.unlock()
                raise

            def monitor():
                try:
                    self.wait()
                finally:
                    _host.unlock()
            threading.Thread(target=monitor, daemon=True).start()
        else:
            super().__init__(*args, **kwargs)


def _locked(original):
    def wrapper(*args, **kwargs):
        if _should_lock_command(args, kwargs):
            _host.lock()
            try:
                return original(*args, **kwargs)
            finally:
                _host.unlock()
        return original(*args, **kwargs)
    wrapper.__name__ = original.__name__
    wrapper.__doc__ = original.__doc__
    return wrapper


subprocess.Popen = _LockedPopen
subprocess.run = _locked(_original_run)
subprocess.call = _locked(_original_call)
subprocess.check_call = _locked(_original_check_call)
subprocess.check_output = _locked(_original_check_output)


# --- sudo_manager ----------------------------------------------------------

class SudoManager:
    """The original SudoManager API over the host's shared credential helper."""

    @property
    def user_password(self):
        return _host.sudo_password()

    @user_password.setter
    def user_password(self, password):
        self.set_password(password)

    @property
    def wrapper_path(self):
        return _host.sudo_paths()[0]

    @property
    def askpass_script(self):
        return _host.sudo_paths()[1]

    @property
    def fifo_path(self):
        return _host.sudo_paths()[2]

    @property
    def fifo_dir(self):
        return os.path.dirname(self.fifo_path)

    def validate_password(self, password):
        """Validate password using sudo -S -v"""
        if not password:
            return False
        return _host.sudo_validate_password(str(password))

    def set_password(self, password):
        """Store the validated password"""
        _host.sudo_set_password('' if password is None else str(password))

    def run_privileged(self, cmd, **kwargs):
        """Run a command using the sudo wrapper with secure gating"""
        if not _host.sudo_has_password():
            raise ValueError("No password set")
        _host.sudo_allow_feed()
        try:
            full_cmd = [self.wrapper_path] + cmd
            return subprocess.run(full_cmd, **kwargs)
        finally:
            _host.sudo_drain()

    def start_privileged_session(self):
        """Open the password gate for a long-running session"""
        _host.sudo_start_session()

    def stop_privileged_session(self):
        """Close the password gate"""
        _host.sudo_stop_session()

    def clear_cache(self):
        """Invalidate sudo credentials cache"""
        _host.sudo_clear_cache()

    def forget_password(self):
        """Clear stored password and invalidate sudo cache"""
        _host.sudo_forget_password()

    def get_env(self):
        """Return environment variables needed for the wrapper"""
        return os.environ.copy()

    def cleanup(self):
        """Remove temporary files and clear credentials"""
        _host.sudo_cleanup()


sudo_manager = SudoManager()


# --- Translation helpers injected into widget modules ----------------------

def _translate_with_patterns_helper(text, widget_id):
    if not text:
        return ""
    if not isinstance(text, str):
        return text
    return _host.translate(text, widget_id)


def _transient_widget_id(window):
    try:
        parent = window.get_transient_for()
        if parent and hasattr(parent, 'active_widget_id'):
            return parent.active_widget_id
    except Exception:
        pass
    return None


def _translate_dialog(dialog, widget_id=None):
    """Translate all text in a dialog before presenting it."""
    if widget_id is None:
        widget_id = _transient_widget_id(dialog)
    if widget_id:
        _host.translate_dialog(dialog, widget_id)


def _translate_window(window, widget_id=None):
    """Translate all text in a window before presenting it."""
    if widget_id is None:
        widget_id = _transient_widget_id(window)
    if widget_id:
        _host.translate_window(window, widget_id)


def _inject_helpers(module, selected_key):
    def injected_underscore(message, widget_id=selected_key):
        return _translate_with_patterns_helper(message, widget_id)
    setattr(module, '_', injected_underscore)
    setattr(module, 'sudo_manager', sudo_manager)
    setattr(module, 'translate_dialog', lambda dialog: _translate_dialog(dialog, selected_key))
    setattr(module, 'translate_window', lambda window: _translate_window(window, selected_key))


# --- Metadata scan ---------------------------------------------------------

_GETTEXT_FUNCTIONS = {'_', 'gettext', 'N_'}


def _string_literal(node):
    """A string literal, possibly wrapped in _()/gettext()/N_()."""
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    if isinstance(node, ast.Call) and len(node.args) == 1 and not node.keywords:
        function = node.func
        name = function.id if isinstance(function, ast.Name) else (
            function.attr if isinstance(function, ast.Attribute) else None)
        if name in _GETTEXT_FUNCTIONS:
            return _string_literal(node.args[0])
    return None


def _self_attribute(node):
    if (isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name)
            and node.value.id == 'self'):
        return node.attr
    return None


def _statements(nodes):
    """Every statement in nested statement lists (expressions are skipped:
    they cannot hold the assignments the scan looks for)."""
    for node in nodes:
        yield node
        for field in ('body', 'handlers', 'orelse', 'finalbody', 'cases'):
            children = getattr(node, field, None)
            if isinstance(children, list):
                yield from _statements(children)


def _assignments(class_node):
    """(attribute, value node) for every self.<attribute> (or class-level
    name) assignment in the class, in source order."""
    found = []
    class_level_nodes = {id(node) for node in class_node.body}
    for node in _statements(class_node.body):
        if isinstance(node, ast.Assign):
            targets = node.targets
        elif isinstance(node, ast.AnnAssign) and node.value is not None:
            targets = [node.target]
        else:
            continue
        class_level = id(node) in class_level_nodes
        for target in targets:
            name = _self_attribute(target)
            if name is None and class_level and isinstance(target, ast.Name):
                name = target.id
            if name is not None:
                found.append((node.lineno, node.col_offset, name, node.value))
    found.sort(key=lambda item: item[:2])
    return [(name, value) for _line, _column, name, value in found]


def _module_domain(stem):
    return _host.domain_for_module(stem)


def _first_assignment(assignments, attribute):
    """(literal, value node) of the first literal assignment to attribute."""
    for name, value in assignments:
        if name == attribute:
            literal = _string_literal(value)
            if literal is not None:
                return literal, value
    return None, None


def _first_literal(assignments, attribute):
    return _first_assignment(assignments, attribute)[0]


def _class_metadata(class_node):
    return _class_metadata_ex(class_node)[:4]


def _class_metadata_ex(class_node):
    """_class_metadata plus (name_is_translated, widget_id_is_literal): a name
    wrapped in _()/gettext() is translated by the injected _ when the instance
    is built, and the original derived widget_id from that translated name."""
    assignments = _assignments(class_node)
    widget_name, name_node = _first_assignment(assignments, 'widgetname')
    if not widget_name:
        return None
    icon_path = _first_literal(assignments, 'widgeticon')
    # Indirect assignment (self.widgeticon = self.<var>): prefer file paths
    # over symbolic icon names.
    for name, value in assignments:
        variable = _self_attribute(value) if name == 'widgeticon' else None
        if variable is not None:
            candidate = _first_literal(assignments, variable)
            if candidate and ('/' in candidate or candidate.endswith(('.svg', '.png'))):
                icon_path = candidate
            break
    widget_id = _first_literal(assignments, 'widget_id')
    id_is_literal = widget_id is not None
    if widget_id is None:
        widget_id = widget_name.lower().replace(' ', '_')
    translated = isinstance(name_node, ast.Call)
    return widget_name, icon_path, widget_id, class_node.name, translated, id_is_literal


def _widget_classes(path, extended=False):
    """Metadata of each top-level class assigning widgetname, lazily."""
    try:
        with open(path, 'r', encoding='utf-8') as source:
            tree = ast.parse(source.read(), filename=os.fspath(path))
    except (OSError, SyntaxError, ValueError):
        return
    for node in tree.body:
        if isinstance(node, ast.ClassDef):
            metadata = _class_metadata_ex(node)
            if metadata is not None:
                yield metadata if extended else metadata[:4]


def scan_widget_metadata(path):
    """Original _scan_widget_metadata, via the syntax tree: metadata of the
    first top-level class assigning widgetname, without importing."""
    return next(_widget_classes(path), None)


def scan_widget_metadata_ex(path):
    """scan_widget_metadata with the widget name and id as the instance will
    report them: a _()-wrapped name goes through the module's dictionary
    domain (exactly what the injected _ does), and a derived widget_id comes
    from that translated name."""
    metadata = next(_widget_classes(path, extended=True), None)
    if metadata is None:
        return None
    widget_name, icon_path, widget_id, class_name, translated, id_is_literal = metadata
    if translated:
        widget_name = _translate_with_patterns_helper(widget_name, _module_domain(_module_name(path)))
        if not id_is_literal:
            widget_id = widget_name.lower().replace(' ', '_')
    return widget_name, icon_path, widget_id, class_name


# --- Import and instantiation ----------------------------------------------

def _module_name(path):
    return os.path.splitext(os.path.basename(path))[0]


def import_module(path):
    """Execute a widget file as a fresh module named after its stem (not
    registered in sys.modules, like the original)."""
    spec = importlib.util.spec_from_file_location(_module_name(path), path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot load widget module: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def prepare_module(module, path, selected_key):
    """Original _prepare_module, with the dictionary key chosen by the host."""
    print(f"DEBUG: Injecting translation for {_module_name(path)} using key '{selected_key}'")
    _inject_helpers(module, selected_key)


def _gtk():
    import gi
    gi.require_version('Gtk', '4.0')
    gi.require_version('Adw', '1')
    from gi.repository import Adw, Gtk  # noqa: F401 (registers Adw wrappers)
    return Gtk


def _construct(name, widget_class, filename, window, hide_sidebar, voice_autostart,
               conversation_id):
    """Original constructor fallback chain and _register_widget metadata."""
    try:
        try:
            instance = widget_class(hide_sidebar=hide_sidebar, window=window,
                                    voice_autostart=voice_autostart,
                                    conversation_id=conversation_id)
        except TypeError:
            try:
                instance = widget_class(hide_sidebar=hide_sidebar, window=window)
            except TypeError:
                instance = widget_class()
        if not hasattr(instance, 'widgetname'):
            return name, None, f"{name} in {filename} has no widgetname"
        widget_name = instance.widgetname
        icon_path = getattr(instance, 'widgeticon', None)
        widget_id = getattr(instance, 'widget_id', widget_name.lower().replace(' ', '_'))
        metadata = (str(widget_name), str(icon_path) if icon_path else None, str(widget_id))
    except Exception as error:
        print(f"Failed to instantiate widget in {filename}: {error}")
        traceback.print_exc()
        return name, None, traceback.format_exc()
    _INSTANCES.append(instance)
    return name, instance, metadata


def extract_widgets(module, path, window, hide_sidebar, voice_autostart, conversation_id):
    """Original _extract_widgets_from_module, limited to the classes the scan
    found assigning widgetname (every public Gtk.Widget subclass except
    windows when there are none). Returns (class name, instance or None,
    metadata or error text) per class tried."""
    Gtk = _gtk()
    filename = os.path.basename(path)
    declared = {metadata[3] for metadata in _widget_classes(path)}
    results = []
    for attribute_name in dir(module):
        if attribute_name.startswith('_'):
            continue
        attribute = getattr(module, attribute_name)
        if not isinstance(attribute, type) or not issubclass(attribute, Gtk.Widget):
            continue
        if declared and attribute_name not in declared:
            continue
        if issubclass(attribute, Gtk.Window):
            if declared:
                results.append((attribute_name, None,
                                f"{attribute_name} in {filename} is a window, not a widget"))
            continue
        results.append(_construct(attribute_name, attribute, filename, window,
                                  hide_sidebar, voice_autostart, conversation_id))
    return results


def header_bar_widget(instance):
    """Legacy get_header_bar_widget() for single-widget mode."""
    if not hasattr(instance, 'get_header_bar_widget'):
        return None
    try:
        return instance.get_header_bar_widget() or None
    except Exception as error:
        print(f"Error adding widget header controls: {error}")
        return None


def close_request():
    """Original do_close_request widget cleanup hook."""
    for instance in _INSTANCES:
        if hasattr(instance, '_cleanup_screenshot_tmp'):
            try:
                instance._cleanup_screenshot_tmp()
            except Exception:
                traceback.print_exc()


def _retoggle(button):
    # Deactivate first if already active so re-toggling restarts it.
    if button.get_active():
        button.set_active(False)
    button.set_active(True)


def reactivate_voice(instance):
    if instance is not None and hasattr(instance, 'stt_toggle'):
        _retoggle(instance.stt_toggle)


def reactivate_compact_voice(window):
    if window is not None and hasattr(window, '_mic_btn'):
        _retoggle(window._mic_btn)


def launch_compact_voice(application, path, voice_autostart):
    """Original _launch_compact_voice: helpers are injected before exec."""
    _gtk()
    if not os.path.isfile(path):
        raise FileNotFoundError(f"Error: widget file not found: {path}")
    target = _module_name(path)
    spec = importlib.util.spec_from_file_location(target, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Error: cannot load widget module: {path}")
    module = importlib.util.module_from_spec(spec)
    potential_keys = [target, target.replace('-', '_')]
    if len(target) > 2 and target[1] == '-':
        potential_keys.append(target[2:])
    potential_keys.extend([k.replace('_widget', '').replace('-widget', '')
                           for k in list(potential_keys)])
    selected_key = next((key for key in potential_keys if _host.has_domain(key)),
                        potential_keys[0])
    _inject_helpers(module, selected_key)
    spec.loader.exec_module(module)
    compact_class = getattr(module, 'CompactVoiceWindow', None)
    if compact_class is None:
        raise LookupError("Error: CompactVoiceWindow class not found in widget module")
    window = compact_class(application=application, voice_autostart=voice_autostart)
    _INSTANCES.append(window)
    return window


# --- Host window -----------------------------------------------------------

def wrap(pointer):
    """The PyGObject wrapper of a live GObject pointer (the same wrapper while
    it exists)."""
    _gtk()
    from gi import _gi
    return _gi.pygobject_new_full(pointer, False)


def window_wrapper(pointer):
    window = _WINDOWS.get(pointer)
    if window is None:
        window = _WINDOWS[pointer] = wrap(pointer)
    return window


def _resize_window(window, width, height):
    """Custom resize method that tries different approaches"""
    try:
        window.set_default_size(width, height)
        window.set_size_request(width, height)
        print(f"Window resize to {width}x{height} attempted")
        return True
    except Exception as error:
        print(f"Window resize failed: {error}")
        return False


def _resize_and_center(window, width, height):
    """Resize window and attempt to center it"""
    from gi.repository import GLib
    try:
        window.set_default_size(width, height)
        window.set_size_request(width, height)
        window.set_modal(True)
        GLib.timeout_add(100, lambda: window.set_modal(False))
        print(f"Window resize to {width}x{height} attempted")
        return True
    except Exception as error:
        print(f"Window resize failed: {error}")
        return False


def install_window(pointer, hide_sidebar):
    """Attributes of the original WidgetLoaderWindow that widgets use."""
    window = window_wrapper(pointer)
    window.hide_sidebar = hide_sidebar
    window.active_widget_id = None
    window._compact_mode = False
    window.refresh_widgets = lambda: _host.refresh_widgets(pointer)
    window.resize_window = lambda width, height: _resize_window(window, width, height)
    window.resize_and_center = lambda width, height: _resize_and_center(window, width, height)


# --- Translation dictionaries ----------------------------------------------

def load_translation_file(path):
    """Original _load_translation_file: execute the dictionary in a fresh
    module and return its translations as ordered (key, value) pairs."""
    widget_name = _module_name(path).replace('_dictionary', '')
    language_code = os.path.basename(os.path.dirname(os.path.abspath(path)))
    spec = importlib.util.spec_from_file_location(f"{widget_name}_{language_code}_dict", path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot load dictionary: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return [(key, value) for key, value in module.translations.items()
            if isinstance(key, str) and isinstance(value, str)]
