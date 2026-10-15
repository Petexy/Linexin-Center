//! A desktop shortcut for a widget: `/usr/share/applications/<file stem>.desktop`
//! starting `linexin-center -w <file stem>`, written and removed as root (the
//! GTK window's sudo wrapper, LineXinBar's polkit agent).

use crate::catalog::file_stem;
use linexin_services::sudo;
use std::ffi::OsStr;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const APPLICATIONS_DIR: &str = "/usr/share/applications";

pub(crate) fn shortcut_path(file_name: &str) -> PathBuf {
    Path::new(APPLICATIONS_DIR).join(format!("{}.desktop", file_stem(file_name)))
}

/// Original `_perform_shortcut_creation` desktop entry (fields sanitized
/// against injected lines).
pub(crate) fn desktop_entry(display_name: &str, clean_name: &str, icon: Option<&str>) -> String {
    let icon = match icon.unwrap_or_default() {
        "" => String::new(),
        icon if icon.starts_with('/') => icon.to_owned(),
        icon => format!("/usr/share/icons/{icon}"),
    };
    let sanitize = |s: &str| s.replace('\n', " ").replace('\r', "");
    format!(
        "[Desktop Entry]\nName={}\nExec=linexin-center -w {clean_name}\nIcon={}\nTerminal=false\n\
         Type=Application\nCategories=Utility;Tools;\nStartupNotify=false\nHidden=false\n",
        sanitize(display_name),
        sanitize(&icon),
    )
}

/// Run `argv` through the sudo wrapper; the error text mirrors Python's
/// `CalledProcessError`.
pub(crate) fn run_privileged(argv: &[&str]) -> Result<(), String> {
    let args: Vec<&OsStr> = argv.iter().map(OsStr::new).collect();
    let status = sudo::manager()
        .run_privileged(&args)
        .map_err(|error| error.to_string())?;
    match status.code() {
        Some(0) => Ok(()),
        code => Err(failed(argv, code)),
    }
}

/// Write the entry to a new private temporary file (readable, like any
/// desktop file once moved into place).
pub(crate) fn write_temporary(content: &str) -> Result<PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let path = std::env::temp_dir().join(format!(
        "linexin-shortcut-{}-{nanos}.desktop",
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&path)
        .map_err(|error| error.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|error| error.to_string())?;
    Ok(path)
}

/// What changing a shortcut came to: the heading and the message (with `{}`
/// for the widget's name) to show, `None` where there was nothing to do.
pub(crate) type Outcome = Result<Option<(&'static str, &'static str)>, String>;

/// Create (`create`) or remove the shortcut of `file_name`, as root through
/// `run`, which takes the command's words. The temporary file is removed when
/// the move did not happen.
pub(crate) fn change(
    file_name: &str,
    display_name: &str,
    icon: Option<&str>,
    create: bool,
    run: impl Fn(&[&str]) -> Result<(), String>,
) -> Outcome {
    let target = shortcut_path(file_name).to_string_lossy().into_owned();
    if create {
        let content = desktop_entry(display_name, file_stem(file_name), icon);
        let temporary = write_temporary(&content)?;
        let temporary = temporary.to_string_lossy().into_owned();
        let result = run(&["mv", &temporary, &target]);
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map(|()| Some(("Shortcut Created", "Successfully created shortcut for {}")))
    } else if Path::new(&target).exists() {
        run(&["rm", "-f", &target])
            .map(|()| Some(("Shortcut Removed", "Successfully removed shortcut for {}")))
    } else {
        Ok(None)
    }
}

/// Whether `file_name` has a shortcut.
pub(crate) fn exists(file_name: &str) -> bool {
    shortcut_path(file_name).exists()
}

/// Python's `CalledProcessError` message, which the GTK window shows.
pub(crate) fn failed(argv: &[&str], code: Option<i32>) -> String {
    format!(
        "Command '{}' returned non-zero exit status {}.",
        argv.join(" "),
        code.unwrap_or(-1)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entries_match_the_original() {
        assert_eq!(
            desktop_entry(
                "System\nInformation",
                "z-system-information-widget",
                Some("computer.svg")
            ),
            "[Desktop Entry]\nName=System Information\nExec=linexin-center -w z-system-information-widget\n\
             Icon=/usr/share/icons/computer.svg\nTerminal=false\nType=Application\n\
             Categories=Utility;Tools;\nStartupNotify=false\nHidden=false\n"
        );
        assert!(desktop_entry("A", "a", None).contains("\nIcon=\n"));
        assert!(desktop_entry("A", "a", Some("/opt/a.png")).contains("\nIcon=/opt/a.png\n"));
        assert_eq!(
            shortcut_path("a-system_updater.py"),
            PathBuf::from("/usr/share/applications/a-system_updater.desktop")
        );
    }
}
