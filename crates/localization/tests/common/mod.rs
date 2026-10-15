//! Helpers shared by the oracle tests: the Python side lives in tests/oracle.
#![allow(dead_code)]

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use linexin_localization::{Dictionary, Evaluator};

/// The installed widgets' dictionaries, where Linexin Center is installed.
pub const SYSTEM_LOCALIZATION: &str = "/usr/share/linexin/widgets/localization";

/// Checkouts of the original Python projects to compare against, if any: a
/// `:`-separated list of project checkouts (Linexin-Center, Linpama, ...) or
/// of folders holding them. Unset, the comparisons that need them are skipped.
pub const ORIGINAL_SOURCES: &str = "LINEXIN_ORIGINAL_SOURCES";

/// The original `linexin-center` script, if it is not in a checkout named by
/// [`ORIGINAL_SOURCES`].
pub const ORIGINAL_CENTER: &str = "LINEXIN_ORIGINAL_CENTER";

/// Where a project keeps its dictionaries, and the Center its script.
const PROJECT_LOCALIZATION: &str = "src/usr/share/linexin/widgets/localization";
const PROJECT_CENTER: &str = "src/usr/bin/linexin-center";

pub fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

pub fn unhex(text: &str) -> String {
    let bytes = (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).expect("hex"))
        .collect();
    String::from_utf8(bytes).expect("UTF-8")
}

fn oracle_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/oracle.py")
}

pub fn python_available() -> bool {
    let available = Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !available {
        eprintln!("SKIPPED: python3 unavailable");
    }
    available
}

/// `python3 -B oracle.py <args>` with `input` on stdin; panics on failure.
pub fn oracle(args: &[&str], input: &str) -> String {
    try_oracle(args, input).unwrap_or_else(|error| panic!("oracle {args:?}: {error}"))
}

pub fn try_oracle(args: &[&str], input: &str) -> Result<String, String> {
    let mut child = Command::new("python3")
        .arg("-B")
        .arg(oracle_script())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let input = input.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    let written = writer.join().map_err(|_| "writer panicked")?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    written.map_err(|error| error.to_string())?;
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

/// What the python-bridge's evaluator does: execute the file like the original.
pub fn python_evaluator() -> Box<Evaluator> {
    Box::new(|path: &Path| {
        let output = try_oracle(&["eval", &path.to_string_lossy()], "")?;
        Ok(output
            .lines()
            .map(|line| {
                let (key, value) = line.split_once(' ').expect("pair");
                (unhex(key), unhex(value))
            })
            .collect())
    })
}

/// `"k1:v1 k2:v2"` hex pairs as printed by the oracle.
pub fn parse_pairs(text: &str) -> Dictionary {
    text.split_whitespace()
        .map(|pair| {
            let (key, value) = pair.split_once(':').expect("pair");
            (unhex(key), unhex(value))
        })
        .collect()
}

/// The Center's own dictionaries, as this checkout ships them.
pub fn own_localization() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/localization")
}

/// The original projects named by [`ORIGINAL_SOURCES`]: each entry is a
/// project checkout, or a folder whose children are. An entry that does not
/// exist fails the test rather than quietly comparing less.
pub fn original_projects() -> Vec<PathBuf> {
    let Some(list) = env::var_os(ORIGINAL_SOURCES) else {
        return Vec::new();
    };
    let mut projects = Vec::new();
    for entry in env::split_paths(&list).filter(|entry| !entry.as_os_str().is_empty()) {
        assert!(
            entry.is_dir(),
            "{ORIGINAL_SOURCES} names {}, which is not a folder",
            entry.display()
        );
        if entry.join(PROJECT_LOCALIZATION).is_dir() || entry.join(PROJECT_CENTER).is_file() {
            projects.push(entry);
            continue;
        }
        let mut children: Vec<PathBuf> = fs::read_dir(&entry)
            .into_iter()
            .flatten()
            .flatten()
            .map(|child| child.path())
            .filter(|child| {
                child.join(PROJECT_LOCALIZATION).is_dir() || child.join(PROJECT_CENTER).is_file()
            })
            .collect();
        children.sort();
        projects.extend(children);
    }
    projects
}

/// The original `linexin-center` script: [`ORIGINAL_CENTER`], else the one in
/// a checkout named by [`ORIGINAL_SOURCES`]; `None` when neither says.
pub fn original_center() -> Option<PathBuf> {
    if let Some(path) = env::var_os(ORIGINAL_CENTER) {
        let path = PathBuf::from(path);
        assert!(
            path.is_file(),
            "{ORIGINAL_CENTER} names {}, which is not a file",
            path.display()
        );
        return Some(path);
    }
    original_projects()
        .into_iter()
        .map(|project| project.join(PROJECT_CENTER))
        .find(|script| script.is_file())
}

/// Every `localization` dir to read: the Center's own, the installed
/// widgets' where Linexin Center is installed, and the original projects'.
pub fn localization_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![own_localization(), PathBuf::from(SYSTEM_LOCALIZATION)];
    dirs.extend(
        original_projects()
            .into_iter()
            .map(|project| project.join(PROJECT_LOCALIZATION)),
    );
    dirs.retain(|dir| dir.is_dir());
    dirs.dedup();
    dirs
}

/// `<dir>/*/*.py` of every localization dir.
pub fn dictionary_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in localization_dirs() {
        for language in fs::read_dir(&dir).into_iter().flatten().flatten() {
            for file in fs::read_dir(language.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                let path = file.path();
                if path.extension().is_some_and(|extension| extension == "py") && path.is_file() {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files
}

/// A fresh directory under cargo's per-target temp dir (inside the workspace).
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("linexin-localization")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Writes `<root>/<lang>/<domain>_dictionary.py` files.
pub fn write_dictionaries(root: &Path, files: &[(&str, &str, &str)]) {
    for (language, domain, source) in files {
        let dir = root.join(language);
        fs::create_dir_all(&dir).expect("create language dir");
        fs::write(dir.join(format!("{domain}_dictionary.py")), source).expect("write dictionary");
    }
}
