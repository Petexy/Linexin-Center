#!/usr/bin/env python3
import gi
import subprocess
import threading
import gettext
import locale
import os
import distro
import psutil
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, GLib, Pango, Gdk, Gio

APP_NAME = "system-information"
LOCALE_DIR = os.path.abspath("/usr/share/locale")
locale.setlocale(locale.LC_ALL, '')
locale.bindtextdomain(APP_NAME, LOCALE_DIR)
gettext.bindtextdomain(APP_NAME, LOCALE_DIR)
gettext.textdomain(APP_NAME)
_ = gettext.gettext


class LinexinSysInfoWidget(Gtk.Box):
    def __init__(self, hide_sidebar=False, window=None):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.widgetname = "System Information"
        self.widgeticon = "/usr/share/icons/computer-linexin.svg"
        self.window = window
        self.hide_sidebar = hide_sidebar
        self._section_groups = {}
        self.setup_ui()
        self.load_system_info()
        if self.hide_sidebar and self.window:
            GLib.idle_add(self.resize_window_deferred)

    def resize_window_deferred(self):
        if self.window:
            try:
                self.window.set_default_size(1200, 900)
            except Exception as e:
                print(f"Failed to resize window: {e}")
        return False

    def setup_ui(self):
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_vexpand(True)

        self._clamp = Adw.Clamp()
        self._clamp.set_maximum_size(700)
        self._clamp.set_tightening_threshold(500)

        self._content_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=24)
        self._content_box.set_margin_top(24)
        self._content_box.set_margin_bottom(36)
        self._content_box.set_margin_start(24)
        self._content_box.set_margin_end(24)

        # --- Hero header ---
        self._header_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        self._header_box.set_halign(Gtk.Align.CENTER)
        self._header_box.set_margin_bottom(8)

        self._os_picture = Gtk.Picture()
        self._os_picture.set_can_shrink(False)
        self._os_picture.set_content_fit(Gtk.ContentFit.SCALE_DOWN)
        self._os_picture.set_size_request(192, 192)
        self._os_picture.set_halign(Gtk.Align.CENTER)
        self._header_box.append(self._os_picture)
        self._apply_os_logo()
        style_manager = Adw.StyleManager.get_default()
        style_manager.connect("notify::dark", lambda *_: self._apply_os_logo())

        self._os_label = Gtk.Label(label="")
        self._os_label.add_css_class("title-1")
        self._os_label.set_halign(Gtk.Align.CENTER)
        self._header_box.append(self._os_label)

        try:
            hostname = os.uname().nodename
            hostname_label = Gtk.Label(label=hostname)
            hostname_label.add_css_class("dim-label")
            hostname_label.set_halign(Gtk.Align.CENTER)
            self._header_box.append(hostname_label)
        except Exception:
            pass

        self._content_box.append(self._header_box)

        # --- Section groups (populated after data loads) ---
        self._system_group = Adw.PreferencesGroup()
        self._system_group.set_title(_("System"))
        self._content_box.append(self._system_group)

        self._session_group = Adw.PreferencesGroup()
        self._session_group.set_title(_("Session"))
        self._content_box.append(self._session_group)

        self._hardware_group = Adw.PreferencesGroup()
        self._hardware_group.set_title(_("Hardware"))
        self._content_box.append(self._hardware_group)

        self._storage_group = Adw.PreferencesGroup()
        self._storage_group.set_title(_("Storage & Memory"))
        self._content_box.append(self._storage_group)

        self._clamp.set_child(self._content_box)
        scrolled.set_child(self._clamp)
        self.append(scrolled)

    def _create_action_row(self, title, subtitle, icon_name=None):
        row = Adw.ActionRow()
        row.set_title(title)
        row.set_subtitle(str(subtitle))
        row.set_subtitle_selectable(True)
        if icon_name:
            icon = Gtk.Image.new_from_icon_name(icon_name)
            icon.set_pixel_size(20)
            row.add_prefix(icon)
        return row

    def _create_usage_row(self, title, used, total, percent, icon_name=None):
        row = Adw.ActionRow()
        row.set_title(title)
        row.set_subtitle(f"{self.format_bytes(used)} / {self.format_bytes(total)}")
        row.set_subtitle_selectable(True)
        if icon_name:
            icon = Gtk.Image.new_from_icon_name(icon_name)
            icon.set_pixel_size(20)
            row.add_prefix(icon)

        bar_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        bar_box.set_valign(Gtk.Align.CENTER)

        level_bar = Gtk.LevelBar()
        level_bar.set_min_value(0)
        level_bar.set_max_value(100)
        level_bar.set_value(percent)
        level_bar.set_size_request(120, 8)
        level_bar.set_valign(Gtk.Align.CENTER)
        bar_box.append(level_bar)

        pct_label = Gtk.Label(label=f"{percent:.0f}%")
        pct_label.add_css_class("caption")
        pct_label.add_css_class("dim-label")
        pct_label.set_width_chars(4)
        bar_box.append(pct_label)

        row.add_suffix(bar_box)
        return row

    def _get_logo_base_name(self):
        """Read LOGO from os-release, fall back to 'gnome-logo' (matching GNOME Settings)."""
        try:
            with open('/etc/os-release', 'r') as f:
                for line in f:
                    if line.startswith('LOGO='):
                        logo = line.split('=', 1)[1].strip().strip('"').strip("'")
                        if logo:
                            return logo
        except Exception:
            pass
        return 'gnome-logo'

    def _apply_os_logo(self):
        """Replicate GNOME Settings 50 logo lookup: try -text-dark/-text/-dark/base variants."""
        logo_name = self._get_logo_base_name()
        dark = Adw.StyleManager.get_default().get_dark()
        candidates = []
        if dark:
            candidates.append(f"{logo_name}-text-dark")
        candidates.append(f"{logo_name}-text")
        if dark:
            candidates.append(f"{logo_name}-dark")
        candidates.append(logo_name)
        display = Gdk.Display.get_default()
        icon_theme = Gtk.IconTheme.get_for_display(display)
        scale = self._os_picture.get_scale_factor()
        direction = self._os_picture.get_direction()
        icon = Gio.ThemedIcon.new_from_names(candidates)
        paintable = icon_theme.lookup_by_gicon(icon, 192, scale, direction, 0)
        self._os_picture.set_paintable(paintable)

    def format_bytes(self, bytes_value):
        for unit in ['B', 'KB', 'MB', 'GB', 'TB']:
            if bytes_value < 1024.0:
                return f"{bytes_value:.1f} {unit}"
            bytes_value /= 1024.0
        return f"{bytes_value:.1f} PB"

    def get_cpu_info(self):
        try:
            with open('/proc/cpuinfo', 'r') as f:
                for line in f:
                    if line.startswith('model name'):
                        return line.split(':')[1].strip()
        except Exception:
            pass
        return _("Unknown")

    def get_kernel_info(self):
        try:
            return os.uname().release
        except Exception:
            return _("Unknown")

    def get_uptime(self):
        try:
            import time
            uptime = time.time() - psutil.boot_time()
            days = int(uptime // 86400)
            hours = int((uptime % 86400) // 3600)
            minutes = int((uptime % 3600) // 60)
            if days > 0:
                return f"{days}d {hours}h {minutes}m"
            elif hours > 0:
                return f"{hours}h {minutes}m"
            else:
                return f"{minutes}m"
        except Exception:
            return _("Unknown")

    def get_version_date(self):
        try:
            with open('/version', 'r') as f:
                content = f.read().strip()
                return content if content else None
        except Exception:
            pass
        return None

    def get_version_id(self):
        try:
            with open('/etc/os-release', 'r') as f:
                for line in f:
                    if line.startswith('VERSION_ID='):
                        return line.split('=')[1].strip().strip('"')
        except Exception:
            pass
        return None

    def get_session_type(self):
        session_type = os.environ.get('XDG_SESSION_TYPE', '').lower()
        if session_type:
            return session_type.capitalize()
        return _("Unknown")

    def get_desktop_environment(self):
        for env_var in ['XDG_CURRENT_DESKTOP', 'DESKTOP_SESSION', 'XDG_SESSION_DESKTOP']:
            de = os.environ.get(env_var, '').lower()
            if de:
                de_mapping = {
                    'gnome': 'GNOME',
                    'kde': 'KDE',
                    'xfce': 'Xfce',
                    'mate': 'MATE',
                    'cinnamon': 'Cinnamon',
                    'lxde': 'LXDE',
                    'lxqt': 'LXQt',
                    'pantheon': 'Pantheon',
                    'budgie': 'Budgie',
                    'deepin': 'Deepin',
                    'unity': 'Unity',
                    'i3': 'i3',
                    'sway': 'Sway',
                    'awesome': 'Awesome',
                    'openbox': 'Openbox',
                    'fluxbox': 'Fluxbox',
                    'bspwm': 'bspwm',
                    'dwm': 'dwm',
                    'qtile': 'Qtile',
                    'herbstluftwm': 'herbstluftwm'
                }
                return de_mapping.get(de, de.capitalize())
        return _("Unknown")

    def get_window_manager(self):
        wm = os.environ.get('WINDOW_MANAGER', '')
        if wm:
            return os.path.basename(wm)
        try:
            result = subprocess.run(['ps', 'aux'], capture_output=True, text=True, timeout=5)
            if result.returncode == 0:
                wm_mapping = {
                    'mutter': 'Mutter',
                    'kwin_x11': 'KWin',
                    'kwin_wayland': 'KWin',
                    'kwin': 'KWin',
                    'xfwm4': 'Xfwm4',
                    'openbox': 'Openbox',
                    'i3': 'i3',
                    'sway': 'Sway',
                    'awesome': 'Awesome',
                    'dwm': 'DWM',
                    'bspwm': 'bspwm',
                    'qtile': 'Qtile',
                    'herbstluftwm': 'herbstluftwm',
                    'fluxbox': 'Fluxbox',
                    'marco': 'Marco',
                    'metacity': 'Metacity',
                    'compiz': 'Compiz',
                    'enlightenment': 'Enlightenment',
                    'cwm': 'CWM',
                    'jwm': 'JWM'
                }
                for wm_proc, wm_name in wm_mapping.items():
                    if wm_proc in result.stdout:
                        return wm_name
        except (subprocess.SubprocessError, FileNotFoundError):
            pass
        try:
            if os.environ.get('DISPLAY'):
                result = subprocess.run(['xprop', '-root', '_NET_SUPPORTING_WM_CHECK'],
                                        capture_output=True, text=True, timeout=5)
                if result.returncode == 0 and 'window id' in result.stdout:
                    wm_id = result.stdout.split()[-1]
                    result2 = subprocess.run(['xprop', '-id', wm_id, '_NET_WM_NAME'],
                                             capture_output=True, text=True, timeout=5)
                    if result2.returncode == 0 and '=' in result2.stdout:
                        wm_name = result2.stdout.split('=')[1].strip().strip('"')
                        if wm_name.lower() != 'gnome shell':
                            return wm_name
        except (subprocess.SubprocessError, FileNotFoundError):
            pass
        return _("Unknown")

    def get_gpu_info(self):
        gpu_name = _("Unknown")
        driver_version = None
        try:
            result = subprocess.run(['lspci'], capture_output=True, text=True, timeout=10)
            if result.returncode == 0:
                for line in result.stdout.split('\n'):
                    if 'VGA' in line or 'Display' in line or '3D' in line:
                        if ':' in line:
                            gpu_part = line.split(':', 2)[-1].strip()
                            gpu_part = gpu_part.replace('VGA compatible controller: ', '')
                            gpu_part = gpu_part.replace('Display controller: ', '')
                            gpu_part = gpu_part.replace('3D controller: ', '')
                            if '(rev' in gpu_part:
                                gpu_part = gpu_part.split('(rev')[0].strip()
                            if 'NVIDIA Corporation' in gpu_part:
                                gpu_part = gpu_part.replace('NVIDIA Corporation ', '')
                                if '[' in gpu_part and ']' in gpu_part:
                                    bracket_content = gpu_part[gpu_part.find('[')+1:gpu_part.find(']')]
                                    gpu_name = bracket_content
                                else:
                                    gpu_name = gpu_part
                            elif 'AMD' in gpu_part or 'Advanced Micro Devices' in gpu_part:
                                gpu_part = gpu_part.replace('Advanced Micro Devices, Inc. ', '')
                                gpu_part = gpu_part.replace('AMD ', '')
                                if '[' in gpu_part and ']' in gpu_part:
                                    bracket_content = gpu_part[gpu_part.find('[')+1:gpu_part.find(']')]
                                    gpu_name = bracket_content
                                else:
                                    gpu_name = gpu_part
                            elif 'Intel' in gpu_part:
                                gpu_part = gpu_part.replace('Intel Corporation ', '')
                                gpu_name = gpu_part
                            else:
                                gpu_name = gpu_part
                            break
        except (subprocess.SubprocessError, FileNotFoundError):
            pass
        try:
            result = subprocess.run(['nvidia-smi', '--query-gpu=driver_version', '--format=csv,noheader,nounits'],
                                    capture_output=True, text=True, timeout=5)
            if result.returncode == 0 and result.stdout.strip():
                driver_version = f"NVIDIA {result.stdout.strip()}"
        except (subprocess.SubprocessError, FileNotFoundError):
            pass
        if not driver_version:
            try:
                result = subprocess.run(['modinfo', 'amdgpu'], capture_output=True, text=True, timeout=5)
                if result.returncode == 0:
                    driver_version = "AMDGPU"
            except (subprocess.SubprocessError, FileNotFoundError):
                pass
        if not driver_version:
            try:
                result = subprocess.run(['modinfo', 'i915'], capture_output=True, text=True, timeout=5)
                if result.returncode == 0:
                    driver_version = "Intel i915"
            except (subprocess.SubprocessError, FileNotFoundError):
                pass
        if driver_version:
            return f"{gpu_name} ({driver_version})"
        else:
            return gpu_name

    def load_system_info(self):
        def load_info():
            data = {}
            try:
                os_name = distro.name(pretty=True)
                if not os_name:
                    os_name = f"{distro.id()} {distro.version()}"
                data['os_name'] = os_name

                data['version_id'] = self.get_version_id()
                data['version_date'] = self.get_version_date()
                data['kernel'] = self.get_kernel_info()
                data['uptime'] = self.get_uptime()

                data['session_type'] = self.get_session_type()
                data['desktop_env'] = self.get_desktop_environment()
                data['window_manager'] = self.get_window_manager()

                data['cpu_info'] = self.get_cpu_info()
                data['cpu_count'] = psutil.cpu_count()
                data['gpu_info'] = self.get_gpu_info()

                memory = psutil.virtual_memory()
                data['mem_used'] = memory.used
                data['mem_total'] = memory.total
                data['mem_percent'] = memory.percent

                disk = psutil.disk_usage('/')
                data['disk_used'] = disk.used
                data['disk_total'] = disk.total
                data['disk_percent'] = disk.percent
            except Exception as e:
                print(f"Error loading system info: {e}")
                data['error'] = str(e)

            GLib.idle_add(self.update_ui, data)
        threading.Thread(target=load_info, daemon=True).start()

    def update_ui(self, data):
        if 'error' in data and not data.get('os_name'):
            row = self._create_action_row(
                _("Error"), _("Failed to load system information"), "dialog-error")
            self._system_group.add(row)
            return False

        # Hero header OS name
        self._os_label.set_label(data.get('os_name', ''))

        # System section
        if data.get('version_id'):
            self._system_group.add(self._create_action_row(
                _("Version ID"), data['version_id'], "application-certificate"))
        if data.get('version_date'):
            self._system_group.add(self._create_action_row(
                _("Version Date"), data['version_date'], "preferences-system-time"))
        self._system_group.add(self._create_action_row(
            _("Kernel"), data.get('kernel', _("Unknown")), "application-x-firmware"))
        self._system_group.add(self._create_action_row(
            _("Uptime"), data.get('uptime', _("Unknown")), "preferences-system-time"))

        # Session section
        self._session_group.add(self._create_action_row(
            _("Session Type"), data.get('session_type', _("Unknown")), "preferences-desktop-display"))
        self._session_group.add(self._create_action_row(
            _("Desktop Environment"), data.get('desktop_env', _("Unknown")), "preferences-desktop"))
        self._session_group.add(self._create_action_row(
            _("Window Manager"), data.get('window_manager', _("Unknown")), "preferences-desktop-wallpaper"))

        # Hardware section
        cpu_count = data.get('cpu_count', '')
        cpu_text = data.get('cpu_info', _("Unknown"))
        if cpu_count:
            cpu_text = f"{cpu_text} ({cpu_count} cores)"
        self._hardware_group.add(self._create_action_row(
            _("Processor"), cpu_text, "applications-system"))
        self._hardware_group.add(self._create_action_row(
            _("Graphics"), data.get('gpu_info', _("Unknown")), "video-display"))

        # Storage & Memory section
        self._storage_group.add(self._create_usage_row(
            _("Memory"),
            data.get('mem_used', 0), data.get('mem_total', 0),
            data.get('mem_percent', 0), "drive-harddisk"))
        self._storage_group.add(self._create_usage_row(
            _("Disk Usage"),
            data.get('disk_used', 0), data.get('disk_total', 0),
            data.get('disk_percent', 0), "drive-harddisk"))

        return False


if __name__ == "__main__":
    class TestWindow(Gtk.ApplicationWindow):
        def __init__(self, app):
            super().__init__(application=app)
            self.set_title("System Information Widget")
            self.set_default_size(500, 400)
            widget = LinexinSysInfoWidget(hide_sidebar=True, window=self)
            self.set_child(widget)

    class TestApp(Gtk.Application):
        def do_activate(self):
            window = TestWindow(self)
            window.present()

    app = TestApp()
    app.run()
