//! Install layout and desktop detection (original module-level constants).

use std::path::{Component, Path, PathBuf};

pub const APP_ID: &str = "github.petexy.linexincenter";
pub const SYSTEM_WIDGETS_DIR: &str = "/usr/share/linexin/widgets";

#[derive(Clone, Debug)]
pub struct Paths {
    /// Directories scanned for `*.py` and `*.so` widgets, highest priority first.
    pub widgets_dirs: Vec<PathBuf>,
    /// `<exe dir>/../share`, where icons of a source tree live (original `SCRIPT_DIR`).
    pub share_dir: PathBuf,
}

impl Paths {
    /// The original rule: `<exe dir>/../share/linexin/widgets` when it exists
    /// ("running from source", which is said on stdout), else the system
    /// directory. `overrides` (`--widgets-dir`) replace both.
    pub fn resolve(overrides: &[PathBuf]) -> Self {
        let (paths, source_tree) = Self::locate(overrides);
        if let Some(source_tree) = source_tree {
            say_running_from_source(&source_tree);
        }
        paths
    }

    /// [`Paths::resolve`] without saying anything: also the source tree the
    /// widgets come from, if they do, for the caller to say when it starts.
    pub fn locate(overrides: &[PathBuf]) -> (Self, Option<PathBuf>) {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("/usr/bin"));
        let share_dir = normalize(&exe_dir.join("../share"));
        let mut from_source = None;
        let widgets_dirs = if overrides.is_empty() {
            let source_tree = share_dir.join("linexin/widgets");
            if source_tree.exists() {
                if source_tree != Path::new(SYSTEM_WIDGETS_DIR) {
                    from_source = Some(source_tree.clone());
                }
                vec![source_tree]
            } else {
                vec![PathBuf::from(SYSTEM_WIDGETS_DIR)]
            }
        } else {
            overrides
                .iter()
                .map(|dir| normalize(&absolute(dir)))
                .collect()
        };
        let paths = Self {
            widgets_dirs,
            share_dir,
        };
        (paths, from_source)
    }

    /// `<widgets dir>/localization` of every widgets directory, in priority order.
    pub fn localization_dirs(&self) -> Vec<PathBuf> {
        self.widgets_dirs
            .iter()
            .map(|dir| dir.join("localization"))
            .collect()
    }

    /// Original `_load_widget_icon` path rule: a missing `/usr/share/...` icon is
    /// looked up in the source tree's `share` directory. `None` when no file exists.
    pub fn icon_file(&self, icon: &str) -> Option<PathBuf> {
        if icon.is_empty() {
            return None;
        }
        let path = Path::new(icon);
        if path.exists() {
            return Some(path.to_path_buf());
        }
        let relative = icon.strip_prefix("/usr/share/")?;
        let source = self.share_dir.join(relative);
        source.exists().then(|| normalize(&source))
    }
}

/// Original `_early_detect_de`: Plasma and Hyprland get a plain
/// `Gtk.ApplicationWindow` (server-side decorations) and the Kinexin colors.
pub fn is_plasma_or_hyprland(current_desktop: &str, session_desktop: &str) -> bool {
    let combined = format!("{current_desktop} {session_desktop}").to_uppercase();
    combined.contains("KDE") || combined.contains("PLASMA") || combined.contains("HYPRLAND")
}

pub fn detect_plasma_or_hyprland() -> bool {
    let var = |name| std::env::var(name).unwrap_or_default();
    is_plasma_or_hyprland(&var("XDG_CURRENT_DESKTOP"), &var("XDG_SESSION_DESKTOP"))
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Lexical `os.path.abspath`-style normalization (`..` and `.` removed).
fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other),
        }
    }
    result
}

/// What the original prints when its widgets come from a source tree.
pub fn say_running_from_source(source_tree: &Path) {
    println!(
        "Running from source. Widgets directory: {}",
        source_tree.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_detection_matches_original() {
        assert!(is_plasma_or_hyprland("KDE", ""));
        assert!(is_plasma_or_hyprland("", "plasma"));
        assert!(is_plasma_or_hyprland("Hyprland", ""));
        assert!(!is_plasma_or_hyprland("GNOME", "gnome"));
        assert!(!is_plasma_or_hyprland("", ""));
    }

    #[test]
    fn normalization_is_lexical() {
        assert_eq!(
            normalize(Path::new("/opt/app/bin/../share/./linexin")),
            PathBuf::from("/opt/app/share/linexin")
        );
    }

    #[test]
    fn icons_fall_back_to_the_source_tree() {
        let root = std::env::temp_dir().join(format!("linexin-paths-{}", std::process::id()));
        let icons = root.join("share/icons");
        std::fs::create_dir_all(&icons).unwrap();
        std::fs::write(icons.join("linexin-test-icon.svg"), "<svg/>").unwrap();
        let paths = Paths {
            widgets_dirs: vec![],
            share_dir: root.join("share"),
        };
        assert_eq!(
            paths.icon_file("/usr/share/icons/linexin-test-icon.svg"),
            Some(icons.join("linexin-test-icon.svg"))
        );
        assert_eq!(
            paths.icon_file("/usr/share/icons/linexin-missing.svg"),
            None
        );
        assert_eq!(paths.icon_file("utilities-terminal-symbolic"), None);
        assert_eq!(paths.icon_file(""), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
