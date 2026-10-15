"""Benign legacy plugin for pointer, callback and rendering smoke tests."""
import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk, GLib


class LegacyDemoWidget(Gtk.Box):
    def __init__(self, hide_sidebar=False, window=None):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        self.widgetname = "Legacy Python Demo"
        self.widgeticon = "applications-development-symbolic"
        self.hide_sidebar = hide_sidebar
        self.window = window
        self.click_count = 0
        self.set_margin_top(32)
        self.set_margin_bottom(32)
        self.set_margin_start(32)
        self.set_margin_end(32)
        title = Gtk.Label(label="Legacy Python widget")
        title.add_css_class("title-1")
        self.append(title)
        self.append(Gtk.Label(label="This GTK4 widget is running inside the Rust host."))
        self.status = Gtk.Label(label="0")
        self.status.set_name("linexin-python-demo-count")
        self.append(self.status)
        self.button = Gtk.Button(label="Test Python callback")
        self.button.set_name("linexin-python-demo-increment")
        self.button.connect("clicked", self.on_clicked)
        self.append(self.button)
        self.reset_button = Gtk.Button(label="Reset counter")
        self.reset_button.set_name("linexin-python-demo-reset")
        self.reset_button.connect("clicked", self.on_reset)
        self.append(self.reset_button)
        self.async_status = Gtk.Label(label="pending")
        self.async_status.set_name("linexin-python-demo-idle")
        self.append(self.async_status)
        GLib.timeout_add(30, self.on_timer)

    def on_clicked(self, _button):
        self.click_count += 1
        self.status.set_label(str(self.click_count))

    def on_timer(self):
        self.async_status.set_label("ready")
        return GLib.SOURCE_REMOVE

    def on_reset(self, _button):
        self.click_count = 0
        self.status.set_label("0")
