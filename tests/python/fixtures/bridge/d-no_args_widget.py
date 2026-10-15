"""Fixture: a no-argument constructor next to helper classes the host must
not instantiate (a helper widget and a window)."""
import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk

CONSTRUCTED = []


class HelperRow(Gtk.Box):
    def __init__(self):
        super().__init__()
        CONSTRUCTED.append("HelperRow")


class StandaloneWindow(Gtk.Window):
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        CONSTRUCTED.append("StandaloneWindow")


class NoArgsWidget(Gtk.Box):
    def __init__(self):
        super().__init__()
        self.widgetname = "Fixture No Args"
        CONSTRUCTED.append("NoArgsWidget")
        self.constructed = CONSTRUCTED
