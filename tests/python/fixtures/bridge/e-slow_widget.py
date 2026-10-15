"""Fixture: a module whose import takes a while (background import)."""
import time

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk

time.sleep(1.5)


class SlowWidget(Gtk.Box):
    def __init__(self, hide_sidebar=False, window=None):
        super().__init__()
        self.widgetname = "Fixture Slow"
