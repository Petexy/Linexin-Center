"""Fixture: widgetname is not a literal, so the scan finds nothing and the
host falls back to the original rule (every public Gtk.Widget subclass except
windows)."""
import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk

NAME = "Fixture Dynamic"


class DynamicWidget(Gtk.Box):
    def __init__(self, hide_sidebar=False, window=None):
        super().__init__()
        self.widgetname = NAME


class PlainHelper(Gtk.Label):
    pass


class IgnoredWindow(Gtk.Window):
    pass
