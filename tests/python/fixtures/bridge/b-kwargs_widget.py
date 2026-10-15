"""Fixture: a constructor accepting every host argument (like Alexy AI)."""
import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk


class KwargsWidget(Gtk.Box):
    def __init__(self, hide_sidebar=False, window=None, voice_autostart=False,
                 conversation_id=None, **kwargs):
        super().__init__()
        self.widgetname = "Fixture Kwargs"
        self.widgeticon = "applications-system-symbolic"
        self.received = {
            "hide_sidebar": hide_sidebar,
            "window": window,
            "voice_autostart": voice_autostart,
            "conversation_id": conversation_id,
        }
        self.stt_toggle = Gtk.ToggleButton()
        self.toggles = []
        self.stt_toggle.connect("toggled", lambda button: self.toggles.append(button.get_active()))

    def get_header_bar_widget(self):
        return Gtk.Button(label="Header")
