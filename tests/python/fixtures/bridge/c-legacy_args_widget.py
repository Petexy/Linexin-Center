"""Fixture: the classic (hide_sidebar, window) constructor."""
import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk


class LegacyArgsWidget(Gtk.Box):
    def __init__(self, hide_sidebar=False, window=None):
        super().__init__()
        self.widgetname = "Fixture Legacy Args"
        self.widgeticon = "/usr/share/icons/fixture-legacy.png"
        self.hide_sidebar = hide_sidebar
        self.window = window
        self.cleaned_up = False

    def _cleanup_screenshot_tmp(self):
        self.cleaned_up = True

    def refresh_host(self):
        self.window.refresh_widgets()
