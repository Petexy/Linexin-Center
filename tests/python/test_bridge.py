"""Unit tests for the embedded bridge (crates/python-bridge/python/bridge.py).

Run: python3 -B -m unittest discover -s tests/python -v

The Rust host module is replaced by a recording fake, so no display, sudo or
GTK is needed. End-to-end behaviour (real host services, GTK widgets) is
covered by `cargo test -p linexin-python-bridge` and the bridge_check example.
"""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import sys
import tempfile
import textwrap
import threading
import time
import types
import unittest

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
BRIDGE_FIXTURES = ROOT / "tests/python/fixtures/bridge"
INSTALLED = Path("/usr/share/linexin/widgets")


class FakeHost(types.ModuleType):
    """Records calls; translate marks the domain it was asked for."""

    def __init__(self):
        super().__init__("_linexin_host")
        self.calls = []
        self.depth = 0
        self.max_depth = 0
        self.password = None
        self.wrapper = "/tmp/linexin-sudo-fake"
        self.domains = {"system_information"}

    def translate(self, text, domain):
        self.calls.append(("translate", text, domain))
        return f"[{domain}]{text}"

    def has_domain(self, domain):
        return domain in self.domains

    def domain_for_module(self, stem):
        return stem.split("-", 1)[-1].replace("-", "_")

    def translate_dialog(self, dialog, domain):
        self.calls.append(("translate_dialog", dialog, domain))

    def translate_window(self, window, domain):
        self.calls.append(("translate_window", window, domain))

    def lock(self):
        self.calls.append(("lock",))
        self.depth += 1
        self.max_depth = max(self.max_depth, self.depth)

    def unlock(self):
        self.calls.append(("unlock",))
        self.depth -= 1

    def refresh_widgets(self, window):
        self.calls.append(("refresh_widgets", window))

    def sudo_paths(self):
        return (self.wrapper, "/tmp/linexin-askpass-fake", "/tmp/linexin-pipe-fake/password_pipe")

    def sudo_password(self):
        return self.password

    def sudo_has_password(self):
        return bool(self.password)

    def sudo_set_password(self, password):
        self.calls.append(("sudo_set_password", password))
        self.password = password or None

    def sudo_validate_password(self, password):
        return password == "correct horse"

    def sudo_is_wrapper_command(self, command):
        name = os.path.basename(self.wrapper)
        return bool(command) and (command == self.wrapper or command.endswith(name)
                                  or self.wrapper in command)

    def __getattr__(self, name):
        if name.startswith("sudo_"):
            return lambda *args: self.calls.append((name, *args))
        raise AttributeError(name)


HOST = FakeHost()
sys.modules["_linexin_host"] = HOST
_spec = importlib.util.spec_from_file_location(
    "linexin_bridge", ROOT / "crates/python-bridge/python/bridge.py")
bridge = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(bridge)


def write(directory, name, source):
    path = Path(directory) / name
    path.write_text(textwrap.dedent(source), encoding="utf-8")
    return path


class ScanTests(unittest.TestCase):
    def scan(self, source):
        with tempfile.TemporaryDirectory() as directory:
            return bridge.scan_widget_metadata(write(directory, "w.py", source))

    def test_scan_never_imports(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "executed"
            path = write(directory, "w.py", f"""
                from missing_dependency import Gtk
                open({str(marker)!r}, "w").close()
                class W(Gtk.Box):
                    def __init__(self):
                        self.widgetname = "Name"
                """)
            self.assertEqual(bridge.scan_widget_metadata(path), ("Name", None, "name", "W"))
            self.assertFalse(marker.exists())
            self.assertFalse((Path(directory) / "__pycache__").exists())

    def test_gettext_wrapped_names(self):
        for call in ('_("Davinci Installer")', 'gettext.gettext("Davinci Installer")',
                     'gettext("Davinci Installer")', "N_('Davinci Installer')"):
            with self.subTest(call=call):
                metadata = self.scan(f"""
                    class W:
                        def __init__(self):
                            self.widgetname = {call}
                    """)
                self.assertEqual(metadata[:3], ("Davinci Installer", None, "davinci_installer"))

    def test_wrapped_names_as_the_instance_reports_them(self):
        with tempfile.TemporaryDirectory() as directory:
            path = write(directory, "x-foo.py", """
                class W:
                    def __init__(self):
                        self.widgetname = _("Bar Baz")
                """)
            self.assertEqual(bridge.scan_widget_metadata_ex(path),
                             ("[foo]Bar Baz", None, "[foo]bar_baz", "W"))
            path = write(directory, "x-foo.py", """
                class W:
                    def __init__(self):
                        self.widgetname = _("Bar Baz")
                        self.widget_id = "bar"
                """)
            self.assertEqual(bridge.scan_widget_metadata_ex(path)[2], "bar")
            path = write(directory, "x-foo.py", """
                class W:
                    def __init__(self):
                        self.widgetname = "Bar Baz"
                """)
            self.assertEqual(bridge.scan_widget_metadata_ex(path), ("Bar Baz", None, "bar_baz", "W"))

    def test_non_literal_names_are_not_metadata(self):
        self.assertIsNone(self.scan("""
            NAME = "x"
            class W:
                def __init__(self):
                    self.widgetname = NAME
                    self.other = f"{NAME}"
            """))

    def test_indirect_icon_prefers_file_paths(self):
        metadata = self.scan("""
            class W:
                def __init__(self):
                    self.widgetname = "W"
                    self.icon = os.path.join("a", "b")
                    self.icon = "/usr/share/icons/w.svg"
                    if ok:
                        self.widgeticon = self.icon
                    else:
                        self.widgeticon = "utilities-terminal-symbolic"
            """)
        self.assertEqual(metadata[1], "/usr/share/icons/w.svg")
        metadata = self.scan("""
            class W:
                def __init__(self):
                    self.widgetname = "W"
                    self.icon = "symbolic-name"
                    self.widgeticon = "fallback-symbolic"
                    self.widgeticon = self.icon
            """)
        self.assertEqual(metadata[1], "fallback-symbolic")

    def test_widget_id_literal_or_derived(self):
        self.assertEqual(self.scan("""
            class W:
                def __init__(self):
                    self.widgetname = "Linexin Hello"
                    self.widget_id = "hello"
            """)[2], "hello")
        self.assertEqual(self.scan("""
            class W:
                widgetname = "Two Words"
            """)[2], "two_words")

    def test_first_top_level_class_with_a_name_wins(self):
        metadata = self.scan("""
            class Helper:
                def __init__(self):
                    self.title = "x"
            class First:
                def __init__(self):
                    self.widgetname = "First"
            class Second:
                def __init__(self):
                    self.widgetname = "Second"
            def factory():
                class Nested:
                    def __init__(self):
                        self.widgetname = "Nested"
            """)
        self.assertEqual(metadata, ("First", None, "first", "First"))
        with tempfile.TemporaryDirectory() as directory:
            path = write(directory, "w.py", "class A:\n    widgetname = 'A'\nclass B:\n    widgetname = 'B'\n")
            self.assertEqual([m[3] for m in bridge._widget_classes(path)], ["A", "B"])

    def test_unreadable_sources(self):
        self.assertIsNone(self.scan("class broken(:\n"))
        self.assertIsNone(bridge.scan_widget_metadata("/nonexistent/widget.py"))

    def test_fixtures(self):
        expected = {
            "a-gettext_widget.py": ("Fixture Gettext", "/usr/share/icons/fixture-gettext.svg",
                                    "fixture_gettext", "GettextWidget"),
            "b-kwargs_widget.py": ("Fixture Kwargs", "applications-system-symbolic",
                                   "fixture_kwargs", "KwargsWidget"),
            "c-legacy_args_widget.py": ("Fixture Legacy Args", "/usr/share/icons/fixture-legacy.png",
                                        "fixture_legacy_args", "LegacyArgsWidget"),
            "d-no_args_widget.py": ("Fixture No Args", None, "fixture_no_args", "NoArgsWidget"),
            "h-dynamic_name_widget.py": None,
        }
        for name, metadata in expected.items():
            with self.subTest(name=name):
                self.assertEqual(bridge.scan_widget_metadata(BRIDGE_FIXTURES / name), metadata)

    @unittest.skipUnless(INSTALLED.is_dir(), "no installed widgets")
    def test_installed_widgets_match_the_original_scan(self):
        expected = {
            "aa-alexy-ai-widget.py": ("Alexy AI", "/usr/share/icons/github.petexy.alexy.svg", "alexy_ai"),
            "c-davinci_installer.py": ("DaVinci Installer",
                                       "/usr/share/icons/github.petexy.davinciinstaller.png",
                                       "davinci_installer"),
            "z-linexin-hello-widget.py": ("Linexin Hello", "/usr/share/icons/linexin-hello.png",
                                          "linexin_hello"),
            "z-system-information-widget.py": ("System Information",
                                               "/usr/share/icons/computer-linexin.svg",
                                               "system_information"),
        }
        for name, metadata in expected.items():
            if (INSTALLED / name).exists():
                with self.subTest(name=name):
                    self.assertEqual(bridge.scan_widget_metadata(INSTALLED / name)[:3], metadata)


class SubprocessPatchTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.wrapper = write(self.directory.name, "linexin-sudo-test", "#!/bin/sh\nexit 0\n")
        self.wrapper.chmod(0o700)
        HOST.wrapper = str(self.wrapper)
        HOST.calls.clear()
        HOST.depth = HOST.max_depth = 0

    def tearDown(self):
        self.directory.cleanup()

    def wait_unlocked(self):
        deadline = time.monotonic() + 5
        while HOST.depth and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertEqual(HOST.depth, 0)

    def test_should_lock_command(self):
        wrapper = str(self.wrapper)
        lock = bridge._should_lock_command
        self.assertTrue(lock(([wrapper, "true"],), {}))
        self.assertTrue(lock(((wrapper, "true"),), {}))
        self.assertTrue(lock((), {"args": [wrapper]}))
        self.assertTrue(lock((f"{wrapper} pacman -Syu",), {}))
        self.assertTrue(lock((f"env A=1 {wrapper} true",), {}))
        self.assertTrue(lock((["/elsewhere/linexin-sudo-test"],), {}))
        self.assertFalse(lock((["ls", wrapper],), {}))
        self.assertFalse(lock((["sudo", "true"],), {}))
        self.assertFalse(lock(("",), {}))
        self.assertFalse(lock(([],), {}))
        self.assertFalse(lock((), {}))
        self.assertFalse(lock((self.wrapper,), {}), "a bare PathLike is not inspected")

    def test_run_and_friends_lock_only_privileged_commands(self):
        import subprocess
        self.assertEqual(subprocess.run(["true"]).returncode, 0)
        self.assertEqual(subprocess.check_output(["echo", "x"]), b"x\n")
        self.assertEqual(HOST.max_depth, 0)
        for call in (subprocess.run, subprocess.call, subprocess.check_call, subprocess.check_output):
            with self.subTest(call=call.__name__):
                HOST.max_depth = 0
                call([str(self.wrapper)])
                self.wait_unlocked()
                self.assertGreaterEqual(HOST.max_depth, 1)

    def test_popen_unlocks_when_the_process_exits(self):
        import subprocess
        process = subprocess.Popen([str(self.wrapper)])
        self.assertEqual(HOST.depth, 1)
        process.wait()
        self.wait_unlocked()

    def test_popen_failure_releases_the_lock(self):
        import subprocess
        self.wrapper.unlink()
        with self.assertRaises(FileNotFoundError):
            subprocess.Popen([str(self.wrapper)])
        self.assertEqual(HOST.depth, 0)


class SudoManagerTests(unittest.TestCase):
    def setUp(self):
        HOST.calls.clear()
        HOST.password = None
        HOST.depth = HOST.max_depth = 0
        self.directory = tempfile.TemporaryDirectory()
        self.wrapper = write(self.directory.name, "linexin-sudo-run", '#!/bin/sh\nexit "$1"\n')
        self.wrapper.chmod(0o700)
        HOST.wrapper = str(self.wrapper)

    def tearDown(self):
        self.directory.cleanup()

    def test_paths_and_password(self):
        manager = bridge.sudo_manager
        self.assertEqual(manager.wrapper_path, str(self.wrapper))
        self.assertEqual(manager.askpass_script, "/tmp/linexin-askpass-fake")
        self.assertEqual(manager.fifo_path, "/tmp/linexin-pipe-fake/password_pipe")
        self.assertEqual(manager.fifo_dir, "/tmp/linexin-pipe-fake")
        self.assertIsNone(manager.user_password)
        manager.set_password("pw")
        self.assertEqual(manager.user_password, "pw")
        manager.user_password = None
        self.assertIsNone(manager.user_password)
        self.assertEqual(HOST.calls, [("sudo_set_password", "pw"), ("sudo_set_password", "")])
        self.assertTrue(manager.validate_password("correct horse"))
        self.assertFalse(manager.validate_password(""))
        self.assertFalse(manager.validate_password(None))
        self.assertEqual(manager.get_env(), dict(os.environ))
        self.assertIsNot(manager.get_env(), os.environ)
        self.assertTrue(manager, "widgets test `if sudo_manager:`")

    def test_run_privileged_requires_a_password(self):
        with self.assertRaisesRegex(ValueError, "No password set"):
            bridge.sudo_manager.run_privileged(["true"])
        self.assertEqual(HOST.calls, [])

    def test_run_privileged_feeds_once_and_locks(self):
        HOST.password = "pw"
        result = bridge.sudo_manager.run_privileged(["3"], capture_output=True)
        self.assertEqual(result.returncode, 3)
        self.assertEqual(result.args, [str(self.wrapper), "3"])
        names = [call[0] for call in HOST.calls]
        self.assertEqual(names[0], "sudo_allow_feed")
        self.assertEqual(names[-1], "sudo_drain")
        self.assertIn("lock", names)
        self.assertEqual(HOST.depth, 0)

    def test_run_privileged_drains_on_errors(self):
        HOST.password = "pw"
        with self.assertRaises(TypeError):
            bridge.sudo_manager.run_privileged(("not", "a", "list"))
        self.assertEqual([call[0] for call in HOST.calls], ["sudo_allow_feed", "sudo_drain"])

    def test_sessions_and_cleanup_delegate(self):
        manager = bridge.sudo_manager
        manager.start_privileged_session()
        manager.stop_privileged_session()
        manager.clear_cache()
        manager.forget_password()
        manager.cleanup()
        self.assertEqual([call[0] for call in HOST.calls], [
            "sudo_start_session", "sudo_stop_session", "sudo_clear_cache",
            "sudo_forget_password", "sudo_cleanup"])


class InjectionTests(unittest.TestCase):
    def setUp(self):
        HOST.calls.clear()

    def test_injected_underscore_is_bound_to_the_domain(self):
        module = types.ModuleType("w")
        bridge._inject_helpers(module, "system_information")
        self.assertEqual(module._("Memory"), "[system_information]Memory")
        self.assertEqual(module._("Memory", "other"), "[other]Memory")
        self.assertEqual(module._(""), "")
        self.assertEqual(module._(None), "")
        self.assertEqual(module._(5), 5)
        self.assertIs(module.sudo_manager, bridge.sudo_manager)
        module.translate_dialog("dialog")
        module.translate_window("window")
        self.assertIn(("translate_dialog", "dialog", "system_information"), HOST.calls)
        self.assertIn(("translate_window", "window", "system_information"), HOST.calls)

    def test_prepare_module_overrides_module_level_translators(self):
        module = types.ModuleType("w")
        module._ = str.upper
        with contextlib.redirect_stdout(io.StringIO()) as out:
            bridge.prepare_module(module, "/x/z-system-information-widget.py", "system_information")
        self.assertEqual(out.getvalue(), "DEBUG: Injecting translation for "
                         "z-system-information-widget using key 'system_information'\n")
        self.assertEqual(module._("a"), "[system_information]a")

    def test_dialog_domain_from_the_transient_parent(self):
        parent = types.SimpleNamespace(active_widget_id="package_manager")
        dialog = types.SimpleNamespace(get_transient_for=lambda: parent)
        bridge._translate_dialog(dialog)
        orphan = types.SimpleNamespace(get_transient_for=lambda: None)
        bridge._translate_dialog(orphan)
        bridge._translate_window(object())
        self.assertEqual(HOST.calls, [("translate_dialog", dialog, "package_manager")])


class ConstructorTests(unittest.TestCase):
    def construct(self, cls):
        return bridge._construct(cls.__name__, cls, "w.py", "WINDOW", True, True, "c1")

    def test_fallback_chain(self):
        class AllArguments:
            def __init__(self, hide_sidebar=False, window=None, **kwargs):
                self.widgetname = "All Arguments"
                self.args = (hide_sidebar, window, kwargs)

        class Forwarding:
            def __init__(self, hide_sidebar=False, window=None, **kwargs):
                if kwargs:
                    raise TypeError("unexpected property")
                self.widgetname = "Forwarding"
                self.args = (hide_sidebar, window)

        class Bare:
            def __init__(self):
                self.widgetname = "Bare"
                self.widget_id = "bare_id"
                self.widgeticon = "/i.png"
                self.args = ()

        name, instance, metadata = self.construct(AllArguments)
        self.assertEqual(instance.args, (True, "WINDOW", {"voice_autostart": True, "conversation_id": "c1"}))
        self.assertEqual(metadata, ("All Arguments", None, "all_arguments"))
        self.assertEqual(self.construct(Forwarding)[1].args, (True, "WINDOW"))
        self.assertEqual(self.construct(Bare)[2], ("Bare", "/i.png", "bare_id"))
        self.assertIn(instance, bridge._INSTANCES)

    def test_failures_are_reported(self):
        class Raises:
            def __init__(self, hide_sidebar=False, window=None):
                raise RuntimeError("boom")

        class Nameless:
            pass

        with contextlib.redirect_stdout(io.StringIO()) as out, \
                contextlib.redirect_stderr(io.StringIO()) as err:
            name, instance, error = self.construct(Raises)
        self.assertIn("Failed to instantiate widget in w.py: boom", out.getvalue())
        self.assertIn("RuntimeError: boom", err.getvalue())
        self.assertIsNone(instance)
        self.assertIn("RuntimeError: boom", error)
        self.assertIn("has no widgetname", self.construct(Nameless)[2])


class DictionaryTests(unittest.TestCase):
    def test_executed_dictionary_keeps_dict_order(self):
        with tempfile.TemporaryDirectory() as directory:
            language = Path(directory) / "pl_PL"
            language.mkdir()
            path = write(language, "demo_dictionary.py", """
                PROMPT = "computed"
                translations = {"b": "B", "a": "A", "b": "B2", 3: "skip"}
                translations.update({"c": PROMPT})
                assert __name__ == "demo_pl_PL_dict"
                """)
            self.assertEqual(bridge.load_translation_file(str(path)),
                             [("b", "B2"), ("a", "A"), ("c", "computed")])
            self.assertFalse((language / "__pycache__").exists())

    def test_missing_translations_raise(self):
        with tempfile.TemporaryDirectory() as directory:
            path = write(directory, "x_dictionary.py", "other = {}\n")
            with self.assertRaises(AttributeError):
                bridge.load_translation_file(str(path))


class ImportTests(unittest.TestCase):
    def test_import_is_a_fresh_unregistered_module(self):
        with tempfile.TemporaryDirectory() as directory:
            path = write(directory, "z-some-widget.py", "VALUE = __name__\n")
            first = bridge.import_module(str(path))
            second = bridge.import_module(str(path))
            self.assertEqual(first.VALUE, "z-some-widget")
            self.assertIsNot(first, second)
            self.assertNotIn("z-some-widget", sys.modules)
            self.assertEqual(first.__file__, str(path))

    def test_import_errors_propagate(self):
        with self.assertRaises(ZeroDivisionError):
            bridge.import_module(str(BRIDGE_FIXTURES / "g-import_error.py"))

    def test_install_window_refresh_goes_to_the_host(self):
        window = types.SimpleNamespace()
        bridge._WINDOWS[1234] = window
        bridge.install_window(1234, True)
        self.assertTrue(window.hide_sidebar)
        self.assertIsNone(window.active_widget_id)
        HOST.calls.clear()
        window.refresh_widgets()
        self.assertEqual(HOST.calls, [("refresh_widgets", 1234)])


if __name__ == "__main__":
    unittest.main()
