//! detect_language against the original `_get_system_language` run by python3
//! under the same environment. harness = false: the environment is process
//! global, so every case runs sequentially on the main thread.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use linexin_localization::detect_language;

const VARS: [&str; 7] = [
    "LC_ALL",
    "LC_CTYPE",
    "LANG",
    "LC_MESSAGES",
    "LANGUAGE",
    "PYTHONCOERCECLOCALE",
    "LOCPATH",
];

/// Extra locales compiled into a LOCPATH directory: `(source, charmap, name)`.
/// They exercise names this system does not have installed and glibc's
/// non-archive lookup.
const EXTRA_LOCALES: &[(&str, &str, &str)] = &[
    ("de_DE", "UTF-8", "de_DE.utf8"),
    ("de_DE@euro", "ISO-8859-15", "de_DE@euro"),
    ("sr_RS@latin", "UTF-8", "sr_RS.utf8@latin"),
    ("pt_BR", "ISO-8859-1", "pt_BR"),
    ("ru_RU", "KOI8-R", "ru_RU.koi8r"),
    ("hi_IN", "UTF-8", "hi_IN.utf8"),
    ("en_US", "UTF-8", "en_US.UTF-8"),
];

const LOCALES: &[&str] = &[
    "",
    "C",
    "POSIX",
    "C.UTF-8",
    "C.utf8",
    "UTF-8",
    // The ten supported languages, as Linux spells them.
    "de_DE",
    "de_DE.UTF-8",
    "de_DE.utf8",
    "de_DE@euro",
    "de_DE.UTF-8@euro",
    "de_DE.ISO-8859-1",
    "en_US",
    "en_US.UTF-8",
    "en_US.utf8",
    "en_US.ISO-8859-1",
    "en_US.iso88591",
    "es_ES",
    "es_ES.UTF-8",
    "es_ES@euro",
    "fr_FR.UTF-8",
    "fr_FR.utf8",
    "fr_FR@euro",
    "hi_IN",
    "hi_IN.UTF-8",
    "pl_PL",
    "pl_PL.UTF-8",
    "pl_PL.utf8",
    "pl_PL.UTF8",
    "pl_PL.Utf-8",
    "pl_PL.ISO-8859-2",
    "pt_BR",
    "pt_BR.UTF-8",
    "pt_PT.UTF-8",
    "pt_PT@euro",
    "ru_RU.UTF-8",
    "ru_RU.KOI8-R",
    "zh_CN",
    "zh_CN.UTF-8",
    "zh_CN.utf8",
    "zh_CN.GB18030",
    "zh_CN.GB2312",
    // Other common ones.
    "en_GB.UTF-8",
    "en_GB.utf8",
    "en_GB",
    "en_IE@euro",
    "it_IT.UTF-8",
    "nl_NL.UTF-8",
    "ja_JP.UTF-8",
    "ja_JP.eucJP",
    "ko_KR.UTF-8",
    "uk_UA.UTF-8",
    "tr_TR.UTF-8",
    "cs_CZ.UTF-8",
    "sv_SE.UTF-8",
    "zh_TW.UTF-8",
    "sr_RS",
    "sr_RS@latin",
    "sr_RS.UTF-8@latin",
    "ca_ES.UTF-8@valencia",
    "be_BY@latin",
    // Aliases and invalid values.
    "polish",
    "german",
    "english",
    "garbage",
    "xx_YY.UTF-8",
    ".UTF-8",
    "pl_PL.",
    "@euro",
    "pl_PL.UTF-8@foo",
    "/usr/lib/locale/C.utf8",
    "../../etc/passwd",
    "pl_PL.UTF-8;LC_CTYPE=C",
    "en_US.UTF-8 ",
    "a very long locale name that is certainly not installed anywhere.UTF-8",
];

struct Case {
    vars: Vec<(&'static str, String)>,
}

impl Case {
    fn new(vars: &[(&'static str, &str)]) -> Self {
        Self {
            vars: vars
                .iter()
                .map(|&(var, value)| (var, value.to_owned()))
                .collect(),
        }
    }
}

fn cases(locpath: Option<&Path>) -> Vec<Case> {
    let mut cases = vec![Case::new(&[])];
    for &locale in LOCALES {
        cases.push(Case::new(&[("LANG", locale)]));
        cases.push(Case::new(&[("LC_ALL", locale), ("LANG", "pl_PL.UTF-8")]));
        cases.push(Case::new(&[("LC_CTYPE", locale), ("LANG", "en_GB.UTF-8")]));
        cases.push(Case::new(&[("LANG", locale), ("PYTHONCOERCECLOCALE", "0")]));
        if let Some(locpath) = locpath.and_then(Path::to_str) {
            cases.push(Case::new(&[("LOCPATH", locpath), ("LANG", locale)]));
            cases.push(Case::new(&[
                ("LOCPATH", locpath),
                ("LC_ALL", locale),
                ("LANG", "de_DE.UTF-8"),
            ]));
        }
    }
    for vars in [
        vec![("LC_ALL", ""), ("LC_CTYPE", ""), ("LANG", "")],
        vec![("LC_ALL", "C"), ("LANG", "de_DE.UTF-8")],
        vec![("LC_ALL", "C"), ("LANG", "sr_RS@latin.UTF-8")],
        vec![("LC_ALL", "C")],
        vec![("LC_ALL", "garbage"), ("LANG", ".UTF-8")],
        vec![
            ("LC_ALL", "en_GB.UTF-8"),
            ("LC_CTYPE", "pl_PL.UTF-8"),
            ("LANG", "C"),
        ],
        vec![("LC_CTYPE", "pl_PL.UTF-8"), ("LANG", "C")],
        vec![("LC_CTYPE", "C"), ("LANG", "pl_PL.UTF-8")],
        vec![("LC_CTYPE", "garbage"), ("LANG", "pl_PL.UTF-8")],
        vec![("LC_MESSAGES", "pl_PL.UTF-8"), ("LANG", "en_GB.UTF-8")],
        vec![("LANGUAGE", "pl:de"), ("LANG", "C.UTF-8")],
        vec![("LANG", "C"), ("PYTHONCOERCECLOCALE", "warn")],
        vec![("LANG", "C"), ("PYTHONCOERCECLOCALE", "1")],
        vec![("LANG", "C"), ("PYTHONCOERCECLOCALE", "")],
        vec![("LC_ALL", ""), ("LANG", "C"), ("PYTHONCOERCECLOCALE", "0")],
    ] {
        cases.push(Case::new(&vars));
    }
    cases
}

/// Compiles [`EXTRA_LOCALES`] once into cargo's temp dir; `None` without localedef.
fn compile_locales() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("linexin-localization/locales");
    std::fs::create_dir_all(&dir).ok()?;
    for (source, charmap, name) in EXTRA_LOCALES {
        let target = dir.join(name);
        if !target.is_dir() {
            // Exit status 1 only reports warnings; the directory tells success.
            let _ = Command::new("localedef")
                .args(["-c", "-i", source, "-f", charmap])
                .arg(&target)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        if !target.is_dir() {
            eprintln!("SKIPPED LOCPATH cases: could not compile {name}");
            return None;
        }
    }
    Some(dir)
}

fn python_language() -> String {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/oracle.py");
    let output = Command::new("python3")
        .arg("-B")
        .arg(script)
        .arg("language")
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .expect("run python3");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("UTF-8")
        .trim_end_matches('\n')
        .to_owned()
}

fn set_environment(case: &Case) {
    // SAFETY: harness = false and no threads are started: only this thread
    // touches the environment.
    unsafe {
        for var in VARS {
            std::env::remove_var(var);
        }
        for (var, value) in &case.vars {
            std::env::set_var(var, value);
        }
    }
}

fn main() {
    if !common::python_available() {
        return;
    }
    let cases = cases(compile_locales().as_deref());
    let mut mismatches = Vec::new();
    let mut results = std::collections::BTreeSet::new();
    for case in &cases {
        set_environment(case);
        let (rust, python) = (detect_language(None), python_language());
        results.insert(python.clone());
        if rust != python {
            mismatches.push(format!("{:?}: rust {rust:?}, python {python:?}", case.vars));
        }
        assert_eq!(detect_language(Some("")), rust, "empty --lang is ignored");
        assert_eq!(detect_language(Some("de_DE")), "de_DE");
        assert_eq!(detect_language(Some("whatever.UTF-8")), "whatever.UTF-8");
    }
    if !mismatches.is_empty() {
        panic!(
            "{} of {} environments differ:\n{}",
            mismatches.len(),
            cases.len(),
            mismatches.join("\n")
        );
    }
    eprintln!(
        "detect_language: {} environments identical to the original (results: {results:?})",
        cases.len()
    );
}
