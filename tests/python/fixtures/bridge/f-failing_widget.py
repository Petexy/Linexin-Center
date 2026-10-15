"""Fixture: a widget whose constructor raises."""
import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk


class FailingWidget(Gtk.Box):
    def __init__(self, hide_sidebar=False, window=None):
        super().__init__()
        self.widgetname = "Fixture Failing"
        raise RuntimeError("fixture constructor failure")
