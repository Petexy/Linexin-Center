//! `~/.config/linexin/linexin-center.json`, shared with the original Center.
//!
//! Like the original, every change re-reads the file, updates its own keys and
//! writes it back, so unknown keys survive.

use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

pub type Config = Map<String, Value>;

pub const WIDGET_ORDER: &str = "widget_order";
pub const HIDDEN_WIDGETS: &str = "hidden_widgets";
pub const COMPACT_MODE: &str = "compact_mode";
pub const HAS_LAUNCHED_BEFORE: &str = "has_launched_before";

#[derive(Clone, Debug)]
pub struct ConfigFile {
    path: PathBuf,
}

impl ConfigFile {
    /// `$XDG_CONFIG_HOME/linexin/linexin-center.json` (default `~/.config`).
    pub fn from_env() -> Self {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"));
        Self::at(base.join("linexin/linexin-center.json"))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The stored object; empty when missing or unreadable (original `_load_config`).
    pub fn load(&self) -> Config {
        fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| match value {
                Value::Object(map) => Some(map),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Original `_save_config`: directory created with mode 0700, errors printed.
    pub fn save(&self, config: &Config) {
        if let Err(error) = self.try_save(config) {
            println!("Error saving config: {error}");
        }
    }

    fn try_save(&self, config: &Config) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        fs::write(&self.path, to_python_json(config)?)
    }

    /// Load, apply `change`, save.
    pub fn update(&self, change: impl FnOnce(&mut Config)) {
        let mut config = self.load();
        change(&mut config);
        self.save(&config);
    }
}

/// Byte-for-byte what the original's `json.dump(config, f, indent=2)` wrote:
/// keys in insertion order, two-space indent, `ensure_ascii` escapes
/// (lowercase `\uXXXX`, surrogate pairs above the BMP), no final newline.
fn to_python_json(config: &Config) -> std::io::Result<String> {
    let text = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    let mut escaped = String::with_capacity(text.len());
    // Outside strings JSON is ASCII, so every non-ASCII char is string content.
    for c in text.chars() {
        if c.is_ascii() {
            escaped.push(c);
        } else {
            for unit in c.encode_utf16(&mut [0; 2]) {
                escaped.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    Ok(escaped)
}

/// Python truthiness of `config.get(key, False)`.
pub fn flag(config: &Config, key: &str) -> bool {
    match config.get(key) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// String items of a list value (`widget_order`, `hidden_widgets`).
pub fn names(config: &Config, key: &str) -> Vec<String> {
    match config.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => vec![],
    }
}

pub fn set_names(config: &mut Config, key: &str, names: &[String]) {
    let items = names.iter().cloned().map(Value::String).collect();
    config.insert(key.to_owned(), Value::Array(items));
}

/// The names under which `file_name` may be listed in `hidden_widgets`: a
/// `<stem>.py` hidden by the Python package also hides the `<stem>.so` that
/// replaced it.
pub fn hidden_names(file_name: &str) -> impl Iterator<Item = String> {
    let legacy = file_name
        .strip_suffix(".so")
        .map(|stem| format!("{stem}.py"));
    std::iter::once(file_name.to_owned()).chain(legacy)
}

/// Map file names saved by the Python package onto the installed files:
/// `<stem>.py` becomes `<stem>.so` when only the native widget is installed, so
/// order and hidden state survive the upgrade. Duplicates are dropped.
pub fn map_legacy_names(names: Vec<String>, installed: &HashSet<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    names
        .into_iter()
        .map(|name| match name.strip_suffix(".py") {
            Some(stem) if !installed.contains(&name) => {
                let native = format!("{stem}.so");
                if installed.contains(&native) {
                    native
                } else {
                    name
                }
            }
            _ => name,
        })
        .filter(|name| seen.insert(name.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("linexin-config-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_or_invalid_files_are_empty() {
        let dir = temp_dir("invalid");
        let file = ConfigFile::at(dir.join("linexin/linexin-center.json"));
        assert!(file.load().is_empty());
        fs::create_dir_all(dir.join("linexin")).unwrap();
        fs::write(file.path(), "[1, 2]").unwrap();
        assert!(file.load().is_empty());
        fs::write(file.path(), "{broken").unwrap();
        assert!(file.load().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn updates_preserve_unknown_keys_and_create_a_private_directory() {
        let dir = temp_dir("update");
        let file = ConfigFile::at(dir.join("linexin/linexin-center.json"));
        file.update(|config| {
            config.insert("custom".into(), json!({"nested": [1, 2]}));
            config.insert(COMPACT_MODE.into(), json!(false));
        });
        let mode = fs::metadata(dir.join("linexin"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        file.update(|config| {
            config.insert(COMPACT_MODE.into(), json!(true));
            set_names(config, WIDGET_ORDER, &["b.so".into(), "a.py".into()]);
        });
        let config = file.load();
        assert_eq!(config["custom"], json!({"nested": [1, 2]}));
        assert!(flag(&config, COMPACT_MODE));
        assert_eq!(names(&config, WIDGET_ORDER), ["b.so", "a.py"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn writes_exactly_what_python_json_dump_wrote() {
        let source = r#"{"widget_order": ["z-system-information-widget.so", "a-system_updater.py"],
            "compact_mode": true, "zeta": {"b": [], "a": {}, "n": 3, "f": 1.5},
            "name": "Zażółć gęślą jaźń 漢字 🙂", "hidden_widgets": [], "x": null}"#;
        let config: Config = serde_json::from_str(source).unwrap();
        let Ok(output) = std::process::Command::new("python3")
            .args([
                "-c",
                "import json, sys; sys.stdout.write(json.dumps(json.loads(sys.argv[1]), indent=2))",
                source,
            ])
            .output()
        else {
            eprintln!("python3 unavailable; skipping");
            return;
        };
        assert_eq!(
            to_python_json(&config).unwrap(),
            String::from_utf8(output.stdout).unwrap()
        );
    }

    #[test]
    fn python_truthiness() {
        let config: Config = serde_json::from_value(json!({
            "t": true, "f": false, "one": 1, "zero": 0, "s": "x", "empty": "", "list": [0], "null": null
        }))
        .unwrap();
        for key in ["t", "one", "s", "list"] {
            assert!(flag(&config, key), "{key}");
        }
        for key in ["f", "zero", "empty", "null", "missing"] {
            assert!(!flag(&config, key), "{key}");
        }
    }

    #[test]
    fn names_ignore_non_strings() {
        let config: Config = serde_json::from_value(
            json!({"hidden_widgets": ["a.py", 3, null, "b.so"], "widget_order": "x"}),
        )
        .unwrap();
        assert_eq!(names(&config, HIDDEN_WIDGETS), ["a.py", "b.so"]);
        assert!(names(&config, WIDGET_ORDER).is_empty());
    }

    #[test]
    fn legacy_python_names_map_to_installed_native_widgets() {
        let installed: HashSet<String> = [
            "z-system-information-widget.so",
            "a-system_updater.py",
            "both.py",
            "both.so",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let saved = [
            "a-system_updater.py",
            "z-system-information-widget.py",
            "removed-widget.py",
            "both.py",
            "z-system-information-widget.so",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(
            map_legacy_names(saved, &installed),
            [
                "a-system_updater.py",
                "z-system-information-widget.so",
                "removed-widget.py",
                "both.py"
            ]
        );
    }
}
