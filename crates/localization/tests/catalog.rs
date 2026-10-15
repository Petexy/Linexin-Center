//! Loading rules of Localization: directory indexing, precedence, evaluator
//! use, diagnostics, recursion guard and the process-wide catalog.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use common::*;
use linexin_localization::{
    Evaluator, FALLBACK_LANGUAGE, HOST_DOMAIN, Localization, global, install, tr,
};

#[test]
fn earlier_base_dirs_win_per_language_and_domain() {
    let first = temp_dir("precedence-first");
    let second = temp_dir("precedence-second");
    write_dictionaries(
        &first,
        &[("pl_PL", "app", "translations = {'a': 'A first'}\n")],
    );
    write_dictionaries(
        &second,
        &[
            (
                "pl_PL",
                "app",
                "translations = {'a': 'A second', 'b': 'B second'}\n",
            ),
            ("en_US", "app", "translations = {'c': 'C fallback'}\n"),
            ("de_DE", "other", "translations = {'x': 'y'}\n"),
        ],
    );
    let missing = PathBuf::from("/nonexistent/linexin/localization");
    let localization = Localization::load(&[missing, first, second], "pl_PL", None);
    assert!(
        localization.diagnostics().is_empty(),
        "{:?}",
        localization.diagnostics()
    );
    assert_eq!(localization.language(), "pl_PL");
    assert_eq!(localization.get_text("a", "app"), "A first");
    // The whole file is shadowed, not merged key by key.
    assert_eq!(localization.get_text("b", "app"), "b");
    assert_eq!(localization.get_text("c", "app"), "C fallback");
    assert!(localization.has_domain("other"));
    assert!(!localization.has_domain("missing"));
    assert_eq!(localization.get_text("x", "other"), "x");
}

#[test]
fn indexed_domains_match_original_file_naming() {
    let root = temp_dir("naming");
    write_dictionaries(
        &root,
        &[
            ("pl_PL", "plain", "translations = {}\n"),
            ("pl_PL", "a_dictionary_b", "translations = {}\n"),
            ("pl_PL", "", "translations = {}\n"),
            ("pl_PL", ".hidden", "translations = {}\n"),
            ("pl_PL", "with-dash.dot", "translations = {}\n"),
            ("__pycache__", "cached", "translations = {}\n"),
            ("xx", "zz", "translations = {}\n"),
        ],
    );
    fs::create_dir_all(root.join("pl_PL/dir_dictionary.py")).unwrap();
    fs::write(root.join("pl_PL/compiled_dictionary.pyc"), "").unwrap();
    fs::write(
        root.join("pl_PL/upper_DICTIONARY.py"),
        "translations = {}\n",
    )
    .unwrap();
    fs::write(root.join("pl_PL/notes.py"), "translations = {}\n").unwrap();
    fs::write(root.join("stray_dictionary.py"), "translations = {}\n").unwrap();
    let localization = Localization::load(std::slice::from_ref(&root), "pl_PL", None);
    let candidates = [
        "plain",
        "a_b",
        "a_dictionary_b",
        "",
        ".hidden",
        "with-dash.dot",
        "cached",
        "zz",
        "dir",
        "compiled",
        "upper_DICTIONARY",
        "upper",
        "notes",
        "stray",
    ];
    let ours: Vec<&str> = candidates
        .into_iter()
        .filter(|domain| localization.has_domain(domain))
        .collect();
    assert_eq!(
        ours,
        [
            "plain",
            "a_b",
            "",
            ".hidden",
            "with-dash.dot",
            "cached",
            "zz"
        ]
    );
    if python_available() {
        let mut python: Vec<String> = oracle(&["domains", &root.to_string_lossy()], "")
            .lines()
            .map(unhex)
            .collect();
        python.sort();
        let mut ours: Vec<String> = ours.into_iter().map(String::from).collect();
        ours.sort();
        assert_eq!(ours, python);
    }
}

#[test]
fn evaluator_runs_only_for_non_literal_active_and_fallback_files() {
    let root = temp_dir("evaluator");
    let code = "translations = dict(x='evaluated')\n";
    write_dictionaries(
        &root,
        &[
            ("pl_PL", "code", code),
            ("en_US", "code", code),
            ("de_DE", "code", code),
            ("pl_PL", "literal", "translations = {'x': 'literal'}\n"),
        ],
    );
    fs::write(
        root.join("pl_PL/latin_dictionary.py"),
        b"translations = {'x': '\xe9'}\n",
    )
    .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    let evaluator = move |path: &Path| {
        recorded.lock().unwrap().push(path.to_path_buf());
        Ok(vec![(
            "x".to_owned(),
            format!(
                "from {}",
                path.parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
            ),
        )])
    };
    let localization = Localization::load(std::slice::from_ref(&root), "pl_PL", Some(&evaluator));
    let mut called = calls.lock().unwrap().clone();
    called.sort();
    assert_eq!(
        called,
        [
            root.join("en_US/code_dictionary.py"),
            root.join("pl_PL/code_dictionary.py"),
            root.join("pl_PL/latin_dictionary.py")
        ]
    );
    assert_eq!(localization.get_text("x", "code"), "from pl_PL");
    assert_eq!(localization.get_text("x", "literal"), "literal");
    assert!(localization.diagnostics().is_empty());

    let without = Localization::load(std::slice::from_ref(&root), "pl_PL", None);
    assert_eq!(without.get_text("x", "code"), "x");
    assert_eq!(
        without.diagnostics().len(),
        3,
        "{:?}",
        without.diagnostics()
    );
    assert!(without.has_domain("code"));

    let failing = |_: &Path| Err("boom".to_owned());
    let failed = Localization::load(&[root], "pl_PL", Some(&failing));
    assert!(
        failed
            .diagnostics()
            .iter()
            .all(|message| message.ends_with("boom"))
    );
    assert_eq!(failed.diagnostics().len(), 3);
}

#[test]
fn python_evaluator_handles_alexy_dictionaries() {
    // Alexy's dictionaries are not the Center's: installed ones, or an
    // original checkout's.
    let alexy = localization_dirs()
        .into_iter()
        .find(|dir| dir.join("pl_PL/aa_alexy_ai_dictionary.py").is_file());
    let Some(dir) = alexy.filter(|_| python_available()) else {
        eprintln!(
            "SKIPPED: no Alexy AI dictionary (installed, or in {ORIGINAL_SOURCES}) or no python3"
        );
        return;
    };
    let dir = dir.as_path();
    let evaluator: Box<Evaluator> = python_evaluator();
    let localization = Localization::load(&[dir.to_path_buf()], "pl_PL", Some(&*evaluator));
    assert!(
        localization.diagnostics().is_empty(),
        "{:?}",
        localization.diagnostics()
    );
    assert_eq!(localization.get_text("Cancel", "aa_alexy_ai"), "Anuluj");
}

#[test]
fn runaway_pattern_recursion_returns_the_input() {
    let root = temp_dir("recursion");
    write_dictionaries(
        &root,
        &[
            ("pl_PL", "loop", "translations = {'{}': '[{}]'}\n"),
            ("pl_PL", "dots", "translations = {'{}.': '<{}>'}\n"),
        ],
    );
    let localization = Localization::load(&[root], "pl_PL", None);
    assert_eq!(localization.translate("anything", "loop"), "anything");
    assert_eq!(localization.translate("", "loop"), "");
    assert_eq!(localization.translate("a..", "dots"), "<<a>>");
    let deep = ".".repeat(5000);
    assert_eq!(localization.translate(&deep, "dots"), deep);
    let nested = ".".repeat(500);
    assert_eq!(
        localization.translate(&nested, "dots"),
        format!("{}{}", "<".repeat(500), ">".repeat(500))
    );
}

#[test]
fn empty_catalog_and_concurrent_use() {
    let empty = Localization::empty("de_DE");
    assert_eq!(empty.language(), "de_DE");
    assert_eq!(
        empty.translate("Downloading {}...", "x"),
        "Downloading {}..."
    );
    assert_eq!(empty.translate_markup("<b>a</b>", "x"), "<b>a</b>");
    assert_eq!(
        empty.domain_for_module("z-system-information-widget"),
        "z-system-information-widget"
    );
    assert!(empty.diagnostics().is_empty());

    let root = temp_dir("threads");
    write_dictionaries(
        &root,
        &[(
            "pl_PL",
            "t",
            "translations = {'Downloading {}...': 'Pobieranie {}...'}\n",
        )],
    );
    let localization = Localization::load(&[root], "pl_PL", None);
    std::thread::scope(|scope| {
        for index in 0..8 {
            let localization = &localization;
            scope.spawn(move || {
                for _ in 0..200 {
                    let text = format!("Downloading {index}...");
                    assert_eq!(
                        localization.translate(&text, "t"),
                        format!("Pobieranie {index}...")
                    );
                }
            });
        }
    });
}

static INSTALLS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn global_catalog_is_installed_once() {
    assert_eq!(
        INSTALLS.fetch_add(1, Ordering::SeqCst),
        0,
        "only this test touches the global catalog"
    );
    let root = temp_dir("global");
    write_dictionaries(
        &root,
        &[(
            "pl_PL",
            HOST_DOMAIN,
            "translations = {'Widgets': 'Widżety'}\n",
        )],
    );
    install(Localization::load(&[root], "pl_PL", None));
    install(Localization::empty(FALLBACK_LANGUAGE));
    assert_eq!(global().language(), "pl_PL");
    assert_eq!(tr("Widgets"), "Widżety");
    assert_eq!(tr("Unknown"), "Unknown");
}
