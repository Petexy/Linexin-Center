"""Fixture: gettext-wrapped name, indirect icon, widget_id, a constructor that
forwards **kwargs to Gtk.Box (rejecting the voice arguments), and the injected
helpers (_, translate_dialog, sudo_manager) used at run time."""
import gettext
import subprocess

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gtk

# Replaced by the host's translator after the module is executed.
_ = gettext.gettext


class GettextWidget(Gtk.Box):
    def __init__(self, hide_sidebar=False, window=None, **kwargs):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, **kwargs)
        self.widgetname = _("Fixture Gettext")
        self.icon_file = "/usr/share/icons/fixture-gettext.svg"
        self.widgeticon = self.icon_file
        self.widget_id = "fixture_gettext"
        self.hide_sidebar = hide_sidebar
        self.window = window
        self.forwarded = kwargs
        self.greeting = _("Hello")
        self.append(Gtk.Label(label=self.greeting))

    def underscore_domain(self):
        return _.__defaults__[0]

    def make_dialog(self):
        dialog = Adw.MessageDialog(heading="Hello", body="World")
        dialog.set_transient_for(self.window)
        translate_dialog(dialog)  # noqa: F821 (injected by the host)
        return dialog

    def run_wrapper(self, *args):
        sudo_manager.start_privileged_session()  # noqa: F821
        try:
            return subprocess.run([sudo_manager.wrapper_path, *args]).returncode  # noqa: F821
        finally:
            sudo_manager.stop_privileged_session()  # noqa: F821

    def run_privileged(self, *args):
        return sudo_manager.run_privileged(list(args)).returncode  # noqa: F821
