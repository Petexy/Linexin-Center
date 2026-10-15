//! Widget files in the widgets directories: discovery, de-duplication, the
//! original ordering rules and `-w` target matching.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Python,
    Native,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetFile {
    /// File name, the key used in the config (`widget_order`, `hidden_widgets`).
    pub file_name: String,
    pub path: PathBuf,
    pub kind: Kind,
    pub modified: SystemTime,
}

impl WidgetFile {
    pub fn stem(&self) -> &str {
        file_stem(&self.file_name)
    }
}

fn kind_of(file_name: &str) -> Option<Kind> {
    if file_name.ends_with(".py") {
        Some(Kind::Python)
    } else if file_name.ends_with(".so") {
        Some(Kind::Native)
    } else {
        None
    }
}

/// File name without its `.py`/`.so` extension.
pub fn file_stem(file_name: &str) -> &str {
    file_name
        .strip_suffix(".py")
        .or_else(|| file_name.strip_suffix(".so"))
        .unwrap_or(file_name)
}

/// All `*.py` and `*.so` files of `dirs`. The first directory wins for equal
/// file names, and a `.so` replaces a `.py` with the same stem (a native port
/// installed over its Python original). Sorted by file name.
pub fn discover(dirs: &[PathBuf]) -> Vec<WidgetFile> {
    let mut seen = HashSet::new();
    let mut files = vec![];
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let mut found: Vec<WidgetFile> = entries
            .flatten()
            .filter_map(|entry| {
                let file_name = entry.file_name().into_string().ok()?;
                let kind = kind_of(&file_name)?;
                let path = entry.path();
                let metadata = fs::metadata(&path).ok().filter(|m| m.is_file())?;
                Some(WidgetFile {
                    modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    file_name,
                    path,
                    kind,
                })
            })
            .collect();
        found.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        files.extend(
            found
                .into_iter()
                .filter(|f| seen.insert(f.file_name.clone())),
        );
    }
    let native_stems: HashSet<String> = files
        .iter()
        .filter(|f| f.kind == Kind::Native)
        .map(|f| f.stem().to_owned())
        .collect();
    files.retain(|f| f.kind == Kind::Native || !native_stems.contains(f.stem()));
    files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    files
}

/// Original `_load_all_widgets_normal` order: saved file names first, the rest
/// by modification time; alphabetical when nothing is saved.
pub fn order(mut files: Vec<WidgetFile>, saved_order: &[String]) -> Vec<WidgetFile> {
    if saved_order.is_empty() {
        files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        return files;
    }
    let mut ordered = vec![];
    for name in saved_order {
        if let Some(index) = files.iter().position(|f| &f.file_name == name) {
            ordered.push(files.remove(index));
        }
    }
    sort_by_mtime(&mut files);
    ordered.extend(files);
    ordered
}

/// The files of `dirs` in sidebar order for the saved `widget_order`, whose
/// legacy `<stem>.py` names count for an installed `<stem>.so`.
pub fn sidebar_order(dirs: &[PathBuf], saved_order: Vec<String>) -> Vec<WidgetFile> {
    let files = discover(dirs);
    let installed: HashSet<String> = files.iter().map(|f| f.file_name.clone()).collect();
    order(
        files,
        &crate::config::map_legacy_names(saved_order, &installed),
    )
}

pub fn sort_by_mtime(files: &mut [WidgetFile]) {
    files.sort_by(|a, b| {
        a.modified
            .cmp(&b.modified)
            .then_with(|| a.file_name.cmp(&b.file_name))
    });
}

/// Original `-w` matching, extended to `.so` files: lower-case, `-` and space
/// count as `_`, and the extension is optional.
pub fn matches_target(file_name: &str, target: &str) -> bool {
    let normalize = |s: &str| s.to_lowercase().replace(['-', ' '], "_");
    let target = target.to_lowercase();
    normalize(file_stem(&file_name.to_lowercase())) == normalize(file_stem(&target))
}

pub fn find_target<'a>(files: &'a [WidgetFile], target: &str) -> Option<&'a WidgetFile> {
    files.iter().find(|f| matches_target(&f.file_name, target))
}

/// Original `_generate_stack_page_name`, also stripping `.so`.
pub fn stack_page_name(file_name: &str) -> String {
    let clean = file_stem(file_name).to_lowercase().replace([' ', '-'], "_");
    format!("widget_{clean}")
}

/// Whether any of `dirs` exists (original `os.path.exists(WIDGET_DIRECTORY)`).
pub fn any_exists(dirs: &[PathBuf]) -> bool {
    dirs.iter().any(|dir| Path::new(dir).exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn file(name: &str, age: u64) -> WidgetFile {
        WidgetFile {
            file_name: name.into(),
            path: PathBuf::from("/w").join(name),
            kind: kind_of(name).unwrap(),
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1000 - age),
        }
    }

    fn names(files: &[WidgetFile]) -> Vec<&str> {
        files.iter().map(|f| f.file_name.as_str()).collect()
    }

    #[test]
    fn alphabetical_without_saved_order() {
        let files = vec![file("z.py", 1), file("a.so", 5), file("m.py", 3)];
        assert_eq!(names(&order(files, &[])), ["a.so", "m.py", "z.py"]);
    }

    #[test]
    fn saved_order_first_then_oldest_new_files() {
        let files = vec![
            file("a.py", 10),
            file("b.so", 1),
            file("c.py", 30),
            file("d.py", 20),
        ];
        let saved = ["d.py".to_owned(), "gone.py".to_owned(), "a.py".to_owned()];
        assert_eq!(
            names(&order(files, &saved)),
            ["d.py", "a.py", "c.py", "b.so"]
        );
    }

    #[test]
    fn targets_match_like_the_original() {
        for target in [
            "a-system-updater",
            "a_system_updater",
            "A-System_Updater.py",
            "a system updater",
            "a-system_updater.so",
        ] {
            assert!(matches_target("a-system_updater.py", target), "{target}");
        }
        assert!(matches_target(
            "z-system-information-widget.so",
            "z-system-information-widget"
        ));
        assert!(matches_target(
            "z-system-information-widget.so",
            "z-system-information-widget.py"
        ));
        assert!(!matches_target("a-system_updater.py", "system_updater"));
        assert!(!matches_target("a-system_updater.py", "a-system"));
    }

    #[test]
    fn stack_page_names() {
        assert_eq!(
            stack_page_name("a-system_updater.py"),
            "widget_a_system_updater"
        );
        assert_eq!(
            stack_page_name("z-system-information-widget.so"),
            "widget_z_system_information_widget"
        );
        assert_eq!(stack_page_name("My Widget"), "widget_my_widget");
    }

    #[test]
    fn discovery_dedupes_names_and_stems() {
        let root = std::env::temp_dir().join(format!("linexin-catalog-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (first, second) = (root.join("first"), root.join("second"));
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(second.join("dir.py")).unwrap();
        for (dir, name) in [
            (&first, "shared.py"),
            (&first, "ported.so"),
            (&second, "shared.py"),
            (&second, "ported.py"),
            (&second, "other.py"),
            (&second, "notes.txt"),
            (&second, "lib.so"),
        ] {
            fs::write(dir.join(name), name).unwrap();
        }
        let files = discover(&[first.clone(), second.clone(), root.join("missing")]);
        assert_eq!(
            names(&files),
            ["lib.so", "other.py", "ported.so", "shared.py"]
        );
        assert_eq!(files[3].path, first.join("shared.py"));
        assert_eq!(files[0].kind, Kind::Native);
        fs::remove_dir_all(&root).unwrap();
    }
}
