"""Fixture: a module providing CompactVoiceWindow (compact voice mode)."""
import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gtk

# Helpers are injected before execution in compact voice mode.
IMPORT_TIME_GREETING = _("Hello")  # noqa: F821
IMPORT_TIME_SUDO = sudo_manager  # noqa: F821


class CompactVoiceWindow(Adw.Window):
    def __init__(self, voice_autostart=False, **kwargs):
        super().__init__(**kwargs)
        self.voice_autostart = voice_autostart
        self._mic_btn = Gtk.ToggleButton()
        self.toggles = []
        self._mic_btn.connect("toggled", lambda button: self.toggles.append(button.get_active()))
        self.set_content(self._mic_btn)
