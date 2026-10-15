//! Localization lookups against the original Python code (tests/oracle/original.py,
//! verbatim excerpts) over the real dictionaries.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use linexin_localization::{Dictionary, FALLBACK_LANGUAGE, Localization, parse_dictionary_literal};

const ALL_LANGUAGES: &[&str] = &[
    "pl_PL", "de_DE", "zh_CN", "en_GB", "en_US", "es_ES", "fr_FR", "hi_IN", "pt_BR", "pt_PT",
    "ru_RU", "C",
];
const KEY_LANGUAGES: &[&str] = &["pl_PL", "de_DE", "zh_CN", "en_GB"];

#[test]
fn vendored_original_is_verbatim() {
    let Some(center) = original_center().filter(|_| python_available()) else {
        eprintln!(
            "SKIPPED: set {ORIGINAL_CENTER} (or {ORIGINAL_SOURCES}) to compare with the original Center"
        );
        return;
    };
    eprint!("{}", oracle(&["check", &center.to_string_lossy()], ""));
}

/// domain → language → dictionary, read like the Center would.
fn dictionaries(dir: &Path) -> BTreeMap<String, BTreeMap<String, Dictionary>> {
    let evaluator = python_evaluator();
    let mut result: BTreeMap<String, BTreeMap<String, Dictionary>> = BTreeMap::new();
    for language in fs::read_dir(dir).into_iter().flatten().flatten() {
        let language_name = language.file_name().to_string_lossy().into_owned();
        for file in fs::read_dir(language.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = file.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix("_dictionary.py") else {
                continue;
            };
            let path = file.path();
            let source = fs::read_to_string(&path).expect("read dictionary");
            let dictionary = parse_dictionary_literal(&source)
                .or_else(|_| evaluator(&path))
                .expect("dictionary loads");
            result
                .entry(stem.to_owned())
                .or_default()
                .insert(language_name.clone(), dictionary);
        }
    }
    result
}

fn corpus(languages: &BTreeMap<String, Dictionary>, language: &str) -> Vec<String> {
    let mut keys: Vec<&String> = Vec::new();
    for language in [language, FALLBACK_LANGUAGE] {
        if let Some(dictionary) = languages.get(language) {
            keys.extend(dictionary.iter().map(|(key, _)| key));
        }
    }
    let plain: Vec<&String> = keys
        .iter()
        .copied()
        .filter(|key| !key.contains("{}"))
        .collect();
    let patterns: Vec<&String> = keys
        .iter()
        .copied()
        .filter(|key| key.contains("{}"))
        .collect();

    let mut fillers: Vec<String> = [
        "X",
        "",
        "42",
        "multi\nline",
        ".*+?()[]{}|^$\\",
        "{}",
        "  padded  ",
    ]
    .map(String::from)
    .to_vec();
    fillers.extend(plain.iter().take(5).map(|key| key.to_string()));
    fillers.extend(patterns.iter().take(3).map(|key| key.replace("{}", "Y")));
    fillers.extend(
        patterns
            .iter()
            .take(2)
            .map(|key| key.replacen("{}", &patterns[0].replace("{}", "Z"), 1)),
    );

    let mut strings: BTreeSet<String> = keys.iter().map(|key| key.to_string()).collect();
    for pattern in &patterns {
        for filler in &fillers {
            strings.insert(pattern.replace("{}", filler));
            strings.insert(pattern.replacen("{}", filler, 1));
        }
    }
    for key in keys.iter().take(25) {
        strings.insert(format!("prefix {key} suffix"));
        strings.insert(format!("{key}\n{key}"));
        strings.insert(format!("<b>{key}</b>"));
        strings.insert(format!(
            "<span size='large'>  {key}\t</span>\n<i>{key}</i> <a href=\"x\">{key}</a>"
        ));
        strings.insert(format!("> {key} <"));
    }
    for fixed in [
        "",
        " ",
        "Untranslated text",
        "Line one\nLine two",
        "Ünïcödé ✓ 中文",
        "{}",
        "{} {}",
        "a{}b",
        "<b></b>",
        "<b> </b>",
        "<<>>",
        "\x1c\x1d",
        "100%",
        "C:\\path\\(x)",
    ] {
        strings.insert(fixed.to_owned());
    }
    strings.into_iter().collect()
}

fn module_stems() -> Vec<String> {
    let mut stems: BTreeSet<String> = fs::read_dir("/usr/share/linexin/widgets")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            Path::new(&entry.file_name())
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .collect();
    for synthetic in [
        "a",
        "ab",
        "a-",
        "-a",
        "a-b",
        "x-",
        "x-y",
        "--",
        "a--b",
        "z-system-information-widget",
        "a-system_updater",
        "y-package_manager",
        "aa-alexy-ai-widget",
        "z-linexin-hello-widget",
        "system-information-widget",
        "widget_loader",
        "w-widget_loader",
        "widget-loader-widget",
        "q-desktop-presets-widget_widget",
        "é-ł-widget",
        "ź-system_information",
        "b-c-davinci-installer",
        "d-affinity_installer-widget",
        "x-widget",
        "_widget",
        "-widget",
        "x-system_information_widget",
    ] {
        stems.insert(synthetic.to_owned());
    }
    stems.into_iter().collect()
}

struct Run {
    language: &'static str,
    requests: Vec<(char, String, String)>,
}

fn rust_answer(
    localization: &Localization,
    (kind, domain, text): &(char, String, String),
) -> String {
    match kind {
        'T' => localization.translate(text, domain),
        'M' => localization.translate_markup(text, domain),
        'G' => localization.get_text(text, domain),
        _ => localization.domain_for_module(text),
    }
}

fn compare_dir(dir: &Path, languages: &'static [&'static str]) -> usize {
    let dictionaries = dictionaries(dir);
    let runs: Vec<Run> = languages
        .iter()
        .map(|&language| {
            let mut requests = Vec::new();
            for (domain, per_language) in dictionaries
                .iter()
                .map(|(d, l)| (d.as_str(), Some(l)))
                .chain([("nonexistent", None)])
            {
                let strings = per_language
                    .map(|l| corpus(l, language))
                    .unwrap_or_else(|| vec!["Cancel".into(), String::new()]);
                for text in strings {
                    for kind in ['T', 'M', 'G'] {
                        requests.push((kind, domain.to_owned(), text.clone()));
                    }
                }
            }
            requests.extend(
                module_stems()
                    .into_iter()
                    .map(|stem| ('D', "-".to_owned(), stem)),
            );
            Run { language, requests }
        })
        .collect();

    let dir_name = dir.to_string_lossy().into_owned();
    std::thread::scope(|scope| {
        let handles: Vec<_> = runs
            .iter()
            .map(|run| {
                let dir_name = dir_name.clone();
                scope.spawn(move || {
                    let input: String = run
                        .requests
                        .iter()
                        .map(|(kind, domain, text)| format!("{kind} {} {}\n", hex(domain), hex(text)))
                        .collect();
                    let output = oracle(&["run", run.language, &dir_name], &input);
                    let evaluator = python_evaluator();
                    let localization = Localization::load(&[PathBuf::from(&dir_name)], run.language, Some(&*evaluator));
                    assert!(localization.diagnostics().is_empty(), "{:?}", localization.diagnostics());
                    let answers: Vec<&str> = output.lines().collect();
                    assert_eq!(answers.len(), run.requests.len());
                    let (mut compared, mut via_patterns) = (0, 0);
                    for (request, answer) in run.requests.iter().zip(answers) {
                        let expected = match answer {
                            // RecursionError in Python: translate leaves the text as is.
                            "E" if request.0 == 'T' => request.2.clone(),
                            "E" => continue,
                            _ => unhex(&answer[1..]),
                        };
                        assert_eq!(
                            rust_answer(&localization, request),
                            expected,
                            "{} {}: {request:?}",
                            dir_name,
                            run.language
                        );
                        compared += 1;
                        via_patterns += usize::from(request.0 == 'T' && expected != localization.get_text(&request.2, &request.1));
                    }
                    eprintln!(
                        "{dir_name} {}: {compared} lookups identical ({via_patterns} translated through {{}} patterns)",
                        run.language
                    );
                    compared
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("oracle thread"))
            .sum()
    })
}

#[test]
fn lookups_match_original_over_real_dictionaries() {
    let dirs = localization_dirs();
    if dirs.is_empty() || !python_available() {
        eprintln!("SKIPPED: no dictionaries or no python3");
        return;
    }
    let mut total = 0;
    for dir in &dirs {
        let languages = if *dir == own_localization() || dir == Path::new(SYSTEM_LOCALIZATION) {
            ALL_LANGUAGES
        } else {
            KEY_LANGUAGES
        };
        total += compare_dir(dir, languages);
    }
    eprintln!(
        "{total} lookups identical to the original over {} localization dirs",
        dirs.len()
    );
}

/// Fallback rules and edge cases the real dictionaries do not cover.
#[test]
fn lookups_match_original_over_synthetic_dictionaries() {
    if !python_available() {
        return;
    }
    let root = temp_dir("synthetic-localization");
    let patterns = r#"translations = {
    "Downloading {}...": "Pobieranie {}...",
    "{} files": "{} plików",
    "Error: {}": "Błąd: {}",
    "Copy {} to {}": "Kopiuj {} do {}",
    "a.b*c{}(d)": "regex-meta {} ok",
    "Line {}\nnext": "Linia {}\nnastępna",
    "Same {}": "Same {}",
    "Cancel": "Anuluj",
    "Files": "Pliki",
    "Echo": "Echo",
}
"#;
    write_dictionaries(
        &root,
        &[
            ("pl_PL", "synthetic", patterns),
            ("de_DE", "synthetic", "translations = {}\n"),
            (
                "en_US",
                "synthetic",
                "translations = {'Fallback {}': 'Fell back: {}', 'Cancel': 'Cancel (en)', '{} files': '{} file(s)'}\n",
            ),
            ("xx_XX", "synthetic", "translations = {'Echo': 'Echo'}\n"),
            (
                "pl_PL",
                "degenerate",
                "translations = {'{}': '[{}]', 'Stop': 'Stój'}\n",
            ),
            (
                "pl_PL",
                "greedy",
                "translations = {'{}: {}': '{} => {}', '{} end': '{} koniec', '<{}>': '«{}»'}\n",
            ),
            (
                "en_US",
                "only_en",
                "translations = {'Only {}': 'Just {}', 'Word': 'Słowo'}\n",
            ),
            (
                "pl_PL",
                "x-dash_widget",
                "translations = {'Dash': 'Myślnik'}\n",
            ),
            ("pl_PL", "dots", "translations = {'{}.': '<{}>'}\n"),
        ],
    );
    let deep: Vec<String> = [10, 500, 2000].map(|count| ".".repeat(count)).to_vec();
    let texts = [
        "",
        "Cancel",
        "Files",
        "Echo",
        "Downloading Firefox...",
        "Downloading 3 files...",
        "12 files",
        "Error: Downloading x...",
        "Error: Error: 5 files",
        "Copy a to b",
        "a.b*cXYZ(d)",
        "a.b*c(d)",
        "Line 7\nnext",
        "Line 7\nnot next",
        "x Downloading y... and Downloading y... again",
        "Same thing",
        "Fallback here",
        "Fallback Cancel",
        "Only one",
        "Word",
        "Stop",
        "Go",
        "a: b: c",
        "x end end",
        "<a>",
        "<<b>>",
        "ends with\n",
        "Multi\nline\ntext",
        "Downloading \n...",
        "Ünïcödé 12 files",
        "<b>Cancel</b> and <i> Files </i>",
        "<span>Downloading x...</span>",
        "> Cancel <",
        ">Echo<",
        "<b>\x1cCancel\u{a0}</b>",
        "\nab",
        "\n",
        "Copy {} to {}",
    ];
    let languages: &[&str] = &["pl_PL", "de_DE", "fr_FR", "xx_XX", "en_US", "C"];
    let domains = [
        "synthetic",
        "degenerate",
        "greedy",
        "only_en",
        "dots",
        "missing",
    ];
    let mut total = 0;
    for &language in languages {
        let mut requests = Vec::new();
        for domain in domains {
            for text in texts.iter().copied().chain(deep.iter().map(String::as_str)) {
                for kind in ['T', 'M', 'G'] {
                    requests.push((kind, domain.to_owned(), text.to_owned()));
                }
            }
        }
        for stem in [
            "x-dash-widget",
            "y-dash_widget",
            "x-dash",
            "q-only-en",
            "q-greedy-widget",
            "degenerate",
        ] {
            requests.push(('D', "-".to_owned(), stem.to_owned()));
        }
        let input: String = requests
            .iter()
            .map(|(kind, domain, text)| format!("{kind} {} {}\n", hex(domain), hex(text)))
            .collect();
        let output = oracle(&["run", language, &root.to_string_lossy()], &input);
        let localization = Localization::load(std::slice::from_ref(&root), language, None);
        assert!(
            localization.diagnostics().is_empty(),
            "{:?}",
            localization.diagnostics()
        );
        for (request, answer) in requests.iter().zip(output.lines()) {
            let expected = match answer {
                "E" if request.0 == 'T' => request.2.clone(),
                "E" => continue,
                _ => unhex(&answer[1..]),
            };
            assert_eq!(
                rust_answer(&localization, request),
                expected,
                "{language}: {request:?}"
            );
            total += 1;
        }
    }
    eprintln!("{total} synthetic lookups identical to the original");
}
