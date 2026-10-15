"""Verbatim excerpts of the original Linexin Center (src/usr/bin/linexin-center,
v2.3.0) that this crate's tests use as their behavior oracle. Text between the
BEGIN/END markers is copied unchanged; `oracle.py check` verifies that each
excerpt still occurs in the original file. Do not edit the excerpts.
"""
import importlib.util
import locale
import os
import re
from pathlib import Path

FALLBACK_LANGUAGE = "en_US"
_language_override = None
LOCALIZATION_BASE_DIR = None
_localization_manager = None


class WidgetLocalizationManager:
    """The original's methods, without the GObject singleton plumbing."""

# BEGIN excerpt
    def _get_system_language(self):
        """Get the system language code."""
        if _language_override:
            return _language_override
        try:
            lang = locale.getlocale()[0]
            if lang:
                lang_code = lang.split('.')[0]
                return lang_code
        except:
            pass
        lang_env = os.environ.get('LANG', '')
        if lang_env:
            lang_code_env: str = lang_env.split('.')[0]
            if lang_code_env:
                return lang_code_env
        return FALLBACK_LANGUAGE
    def _load_all_translations(self):
        """Load all available translations from the directory structure."""
        localization_dir = Path(LOCALIZATION_BASE_DIR)
        if not localization_dir.exists():
            print(f"Localization directory does not exist: {LOCALIZATION_BASE_DIR}")
            return
        for lang_dir in localization_dir.iterdir():
            if not lang_dir.is_dir():
                continue
            language_code = lang_dir.name
            for dict_file in lang_dir.glob("*_dictionary.py"):
                widget_name = dict_file.stem.replace("_dictionary", "")
                self._load_translation_file(widget_name, language_code, dict_file)
    def _load_translation_file(self, widget_name, language_code, file_path):
        """Load a single translation dictionary file."""
        try:
            spec = importlib.util.spec_from_file_location(
                f"{widget_name}_{language_code}_dict", 
                file_path
            )
            if spec is None:
                return
            loader = spec.loader
            if loader is None:
                return
            module = importlib.util.module_from_spec(spec)
            loader.exec_module(module)
            if hasattr(module, 'translations'):
                if widget_name not in self.widget_translations:
                    self.widget_translations[widget_name] = {}
                self.widget_translations[widget_name][language_code] = module.translations
        except Exception:
            pass
    def get_text(self, key, widget_name="widget_loader"):
        """
        Get translated text for a key.
        """
        if widget_name in self.widget_translations:
            widget_trans = self.widget_translations[widget_name]
            if self.system_language in widget_trans:
                if key in widget_trans[self.system_language]:
                    translated = widget_trans[self.system_language][key]
                    return translated
            if FALLBACK_LANGUAGE in widget_trans:
                if key in widget_trans[FALLBACK_LANGUAGE]:
                    return widget_trans[FALLBACK_LANGUAGE][key]
        return key
# END excerpt


# BEGIN excerpt
def get_localization_manager():
    """Get the singleton localization manager."""
    global _localization_manager
    if _localization_manager is None:
        _localization_manager = WidgetLocalizationManager()
    return _localization_manager
def _(key, widget_name="widget_loader"):
    """Convenience function for translations."""
    return get_localization_manager().get_text(key, widget_name)
# END excerpt


# BEGIN excerpt
def _translate_with_patterns_helper(text, widget_id):
    """
    Helper function to translate text that might contain dynamic variables (placeholders).
    Scans dictionary keys containing {} to match format-string patterns dynamically.
    """
    if not text:
        return ""
    translated = _(text, widget_id)
    if translated != text:
        return translated
    # Dynamically scan dictionary keys that contain {} placeholders
    manager = get_localization_manager()
    if widget_id in manager.widget_translations:
        widget_trans = manager.widget_translations[widget_id]
        lang = manager.system_language
        trans_dict = widget_trans.get(lang) or widget_trans.get(FALLBACK_LANGUAGE, {})
        for pattern, translated_pattern in trans_dict.items():
            if '{}' not in pattern:
                continue
            escaped_pattern = re.escape(pattern)
            regex_pattern = escaped_pattern.replace(re.escape("{}"), r"(.*)")
            match = re.search(regex_pattern, text)
            if match and translated_pattern != pattern:
                if match.groups():
                    content = match.group(1)
                    translated_content = _translate_with_patterns_helper(content, widget_id)
                    translated_text = translated_pattern.replace("{}", translated_content, 1)
                    return text.replace(match.group(0), translated_text)
                else:
                    return text.replace(match.group(0), translated_pattern)
    return text
# END excerpt


# Gtk/Adw are injected by `oracle.py gtk`, which needs a display.
# BEGIN excerpt
def _translate_dialog(dialog, widget_id=None):
    """Translate all text in a dialog before presenting it.
    
    Injected into widget modules as 'translate_dialog'.
    If widget_id is None, attempts to resolve from dialog's transient parent.
    """
    if widget_id is None:
        try:
            win = dialog.get_transient_for()
            if win and hasattr(win, 'active_widget_id'):
                widget_id = win.active_widget_id
        except Exception:
            pass
    if not widget_id:
        return
    # Translate heading/body/responses via properties
    if hasattr(dialog, 'get_heading') and dialog.get_heading():
        translated = _translate_with_patterns_helper(dialog.get_heading(), widget_id)
        if translated != dialog.get_heading():
            dialog.set_heading(translated)
    if hasattr(dialog, 'get_body') and dialog.get_body():
        translated = _translate_with_patterns_helper(dialog.get_body(), widget_id)
        if translated != dialog.get_body():
            dialog.set_body(translated)
    # Recursively translate extra_child and all nested widgets
    _translate_widget_recursive_static(dialog, widget_id)

def _translate_window(window, widget_id=None):
    """Translate all text in a PreferencesWindow before presenting it.
    
    Injected into widget modules as 'translate_window'.
    If widget_id is None, attempts to resolve from window's transient parent.
    """
    if widget_id is None:
        try:
            win = window.get_transient_for()
            if win and hasattr(win, 'active_widget_id'):
                widget_id = win.active_widget_id
        except Exception:
            pass
    if not widget_id:
        return
    _translate_widget_recursive_static(window, widget_id)

def _translate_widget_recursive_static(widget, widget_id):
    """Recursively translate a widget and all its children (static version)."""
    try:
        _translate_single_widget_static(widget, widget_id)
        if hasattr(widget, 'get_first_child'):
            child = widget.get_first_child()
            while child:
                _translate_widget_recursive_static(child, widget_id)
                child = child.get_next_sibling()
        elif hasattr(widget, 'get_child'):
            child = widget.get_child()
            if child:
                _translate_widget_recursive_static(child, widget_id)
    except Exception:
        pass

def _translate_single_widget_static(widget, widget_id):
    """Translate text in a single widget (static version for helpers)."""
    try:
        if isinstance(widget, Gtk.Label):
            if widget.get_use_markup():
                markup = widget.get_label()
                if markup:
                    translated_markup = _translate_markup_content_static(markup, widget_id)
                    if translated_markup != markup:
                        widget.set_markup(translated_markup)
            else:
                text = widget.get_text()
                if text:
                    translated = _translate_with_patterns_helper(text, widget_id)
                    if translated != text:
                        widget.set_text(translated)
        elif isinstance(widget, Gtk.Button):
            label = widget.get_label()
            if label:
                translated = _translate_with_patterns_helper(label, widget_id)
                if translated != label:
                    widget.set_label(translated)
        elif isinstance(widget, (Gtk.Entry, Gtk.SearchEntry, Gtk.PasswordEntry)):
            placeholder = widget.get_placeholder_text()
            if placeholder:
                translated = _translate_with_patterns_helper(placeholder, widget_id)
                if translated != placeholder:
                    widget.set_placeholder_text(translated)
        elif isinstance(widget, Adw.ActionRow):
            title = widget.get_title()
            if title:
                translated = _translate_with_patterns_helper(title, widget_id)
                if translated != title:
                    widget.set_title(translated)
            subtitle = widget.get_subtitle()
            if subtitle:
                translated = _translate_with_patterns_helper(subtitle, widget_id)
                if translated != subtitle:
                    widget.set_subtitle(translated)
            if isinstance(widget, Adw.ComboRow):
                model = widget.get_model()
                if model and isinstance(model, Gtk.StringList):
                    n = model.get_n_items()
                    translated_items = []
                    needs_update = False
                    for i in range(n):
                        item = model.get_string(i)
                        t = _translate_with_patterns_helper(item, widget_id) if item else item
                        translated_items.append(t)
                        if t != item:
                            needs_update = True
                    if needs_update:
                        selected = widget.get_selected()
                        model.splice(0, n, translated_items)
                        widget.set_selected(selected)
        elif isinstance(widget, Adw.PreferencesGroup):
            title = widget.get_title()
            if title:
                translated = _translate_with_patterns_helper(title, widget_id)
                if translated != title:
                    widget.set_title(translated)
            desc = widget.get_description()
            if desc:
                translated = _translate_with_patterns_helper(desc, widget_id)
                if translated != desc:
                    widget.set_description(translated)
        elif isinstance(widget, (Adw.PreferencesRow, Adw.PreferencesPage)):
            title = widget.get_title()
            if title:
                translated = _translate_with_patterns_helper(title, widget_id)
                if translated != title:
                    widget.set_title(translated)
        elif isinstance(widget, Adw.StatusPage):
            title = widget.get_title()
            if title:
                translated = _translate_with_patterns_helper(title, widget_id)
                if translated != title:
                    widget.set_title(translated)
            desc = widget.get_description()
            if desc:
                translated = _translate_with_patterns_helper(desc, widget_id)
                if translated != desc:
                    widget.set_description(translated)
        elif isinstance(widget, (Gtk.Window, Adw.ApplicationWindow)):
            title = widget.get_title()
            if title:
                translated = _translate_with_patterns_helper(title, widget_id)
                if translated != title:
                    widget.set_title(translated)
    except Exception:
        pass
# END excerpt


# BEGIN excerpt
def _translate_markup_content_static(markup_text, widget_id):
    """Translate markup content by matching text between XML tags."""
    markup_text = _translate_with_patterns_helper(markup_text, widget_id)
    def replace_text(match):
        text_content = match.group(1)
        if text_content.strip():
            translated = _(text_content.strip(), widget_id)
            if translated != text_content.strip():
                leading_space = text_content[:len(text_content) - len(text_content.lstrip())]
                trailing_space = text_content[len(text_content.rstrip()):]
                return ">" + leading_space + translated + trailing_space + "<"
        return ">" + text_content + "<"
    return re.sub(r'>([^<]+)<', replace_text, markup_text)
# END excerpt


# Dictionary key chosen by WidgetLoaderWindow._prepare_module.
def select_module_key(filepath):
# BEGIN excerpt
        filepath_str: str = str(filepath)
        filename_base: str = os.path.splitext(os.path.basename(filepath_str))[0]
        potential_keys: list[str] = [filename_base]
        if len(filename_base) > 2 and filename_base[1] == '-':
             potential_keys.append(filename_base[2:]) # type: ignore
        underscore_keys: list[str] = [k.replace('-', '_') for k in potential_keys]
        potential_keys.extend(underscore_keys)
        manager = get_localization_manager()
        selected_key = potential_keys[0]
        for key in potential_keys:
            if key in manager.widget_translations:
                selected_key = key
                break
        if selected_key not in manager.widget_translations:
             for key in potential_keys:
                  clean_key: str = key.replace('_widget', '').replace('-widget', '')
                  if clean_key in manager.widget_translations:
                       selected_key = clean_key
                       break
# END excerpt
        return selected_key
