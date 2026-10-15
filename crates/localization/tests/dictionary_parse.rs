//! parse_dictionary_literal against Python (`ast.literal_eval` to classify,
//! the original loader's execution for the expected content).

mod common;

use std::fs;
use std::path::PathBuf;

use common::*;
use linexin_localization::{Dictionary, parse_dictionary_literal};

enum Expected {
    Literal(Dictionary),
    NotLiteral(Dictionary),
    Broken,
}

fn python_results(files: &[PathBuf]) -> Vec<Expected> {
    let input: String = files
        .iter()
        .map(|path| hex(&path.to_string_lossy()) + "\n")
        .collect();
    let output = oracle(&["parse"], &input);
    let results: Vec<Expected> = output
        .lines()
        .map(|line| {
            let (class, pairs) = line.split_once(' ').unwrap_or((line, ""));
            match class {
                "L" => Expected::Literal(parse_pairs(pairs)),
                "X" => Expected::NotLiteral(parse_pairs(pairs)),
                _ => Expected::Broken,
            }
        })
        .collect();
    assert_eq!(results.len(), files.len());
    results
}

fn rust_parse(path: &PathBuf) -> Result<Dictionary, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let source = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    parse_dictionary_literal(&source)
}

#[test]
fn every_dictionary_on_this_system_matches_python() {
    let files = dictionary_files();
    if files.is_empty() || !python_available() {
        eprintln!("SKIPPED: no dictionaries or no python3");
        return;
    }
    let (mut literal, mut not_literal) = (0, 0);
    for (path, expected) in files.iter().zip(python_results(&files)) {
        let actual = rust_parse(path);
        match expected {
            Expected::Literal(dictionary) => {
                assert_eq!(actual.as_ref(), Ok(&dictionary), "{}", path.display());
                literal += 1;
            }
            Expected::NotLiteral(dictionary) => {
                // The statements the subset knows are read natively; anything
                // else is left to the evaluator.
                if let Ok(actual) = &actual {
                    assert_eq!(actual, &dictionary, "{}", path.display());
                }
                not_literal += 1;
            }
            Expected::Broken => {
                assert!(
                    actual.is_err(),
                    "{} must be left to the evaluator",
                    path.display()
                );
                not_literal += 1;
            }
        }
    }
    eprintln!(
        "{} dictionary files: {literal} literal parsed identically, {not_literal} non-literal rejected",
        files.len()
    );
}

/// `(source, must_parse)`: every case must either fail natively or equal
/// Python's result; `must_parse` cases must succeed.
const SYNTHETIC: &[(&str, bool)] = &[
    ("translations = {}\n", true),
    ("translations = {}", true),
    ("translations={'a':'b'}", true),
    ("translations = {\n    'a': 'b',\n}\n", true),
    (
        "translations = {\n    'a': 'b'\n}\n# trailing comment\n\n",
        true,
    ),
    (
        "# -*- coding: utf-8 -*-\n# comment\ntranslations = {'a': 'b'}\n",
        true,
    ),
    (
        "#!/usr/bin/env python3\n# coding=UTF_8\ntranslations = {'a': 'b'}\n",
        true,
    ),
    (
        "\"\"\"Docstring\nover lines.\"\"\"\n\ntranslations = {'a': 'b'}\n",
        true,
    ),
    ("'doc' 'string'\ntranslations = {'a': 'b'}\n", true),
    (
        "translations = {\n  # comment inside\n  'a': 'b',  # after\n\n  \"c\": \"d\",\n}\n",
        true,
    ),
    (
        "translations = {'dup': '1', 'other': 'x', 'dup': '2'}\n",
        true,
    ),
    (
        "translations = {'a' 'b': 'c' \"d\"\n  '''e''', r'f\\n': u'g'}\n",
        true,
    ),
    (
        "translations = {'''multi\nline''': \"\"\"triple \"quoted\" \\\"\"\" \"\"\"}\n",
        true,
    ),
    (
        "translations = {'esc': '\\n\\t\\r\\a\\b\\f\\v\\\\\\'\\\"'}\n",
        true,
    ),
    (
        "translations = {'oct': '\\0\\7\\12\\101\\1234\\777'}\n",
        true,
    ),
    (
        "translations = {'hex': '\\x41\\xe9\\u0142\\U0001F600'}\n",
        true,
    ),
    (
        "translations = {'unknown': '\\d\\w\\8\\9\\q\\ł\\{'}\n",
        true,
    ),
    (
        "translations = {'cont': 'a\\\nb', 'raw': r'a\\\nb'}\n",
        true,
    ),
    ("translations = {r'\\'': R\"\\\"\", U'x': u\"y\"}\n", true),
    (
        "translations = {'{} pattern {}': '{} wzorzec {}', '': ''}\n",
        true,
    ),
    (
        "translations = {'a': 'b', 'c': 'd'}\n\nPROMPT = \"You are\" ' a test'\n\ntranslations.update({\n    'c': 'D',\n    PROMPT: 'Jesteś',\n    'e': PROMPT,\n})\ntranslations.update({})\n",
        true,
    ),
    (
        "translations.update({'a': 'b'})\ntranslations = {}\n",
        false,
    ),
    ("translations = {UNDEFINED: 'a'}\n", false),
    (
        "X = 'a'\ntranslations = {X: X}\nX = 'b'\ntranslations.update({X: 'c'})\n",
        true,
    ),
    (
        "translations = {}\ntranslations.update({'a': 'b'}, c='d')\n",
        false,
    ),
    (
        "translations = {}\ntranslations.update({'a': 'b'}); x = 'y'\n",
        false,
    ),
    ("translations = {'a': 'b'}\r\nx = 1\r\n", false),
    ("translations = {\r\n'a': '''x\r\ny\rz''',\r\n}\r\n", true),
    ("\u{feff}translations = {'bom': 'ok'}\n", true),
    ("translations = {'tab':\t'x',\x0c'ff': 'y'}\n", true),
    (
        "translations = {'ü': 'ż', '中文': '日本語', 'emoji 😀': '✓'}\n",
        true,
    ),
    // Valid Python the native reader leaves to the evaluator.
    ("translations = {'n': '\\N{BULLET}'}\n", false),
    ("translations = {'a': ('b' 'c')}\n", false),
    ("translations = \\\n{'a': 'b'}\n", false),
    ("translations: dict = {'a': 'b'}\n", false),
    ("translations = {'a': 'b'};\n", false),
    (
        "translations = {'a': 'b'}\ntranslations.update({'c': 'd'})\n",
        false,
    ),
    ("x = 'b'\ntranslations = {'a': x}\n", false),
    ("translations = {'a': f'b'}\n", false),
    ("translations = {'a': b'b'}\n", false),
    ("translations = {'a': 1}\n", false),
    ("translations = dict(a='b')\n", false),
    ("translations = {**{'a': 'b'}}\n", false),
    ("# coding: latin-1\ntranslations = {'a': 'b'}\n", false),
    (
        "\n# vim: set fileencoding=ascii :\ntranslations = {'a': 'b'}\n",
        false,
    ),
    ("translations = {'s': '\\ud800'}\n", false),
    ("if True:\n    translations = {'a': 'b'}\n", false),
    ("'doc'\n'second'\ntranslations = {'a': 'b'}\n", false),
    // Not valid Python at all.
    ("  translations = {'a': 'b'}\n", false),
    ("translations = {'a': 'b'\n", false),
    ("translations = {'a': 'b'}}\n", false),
    ("translations = {'a' 'b'}\n", false),
    ("translations = {'a': 'b',,}\n", false),
    ("translations = {,}\n", false),
    ("translations = {'a': 'unterminated}\n", false),
    ("translations = {'a': 'new\nline'}\n", false),
    ("translations = {'a': '\\x4'}\n", false),
    ("translations = {'a': '\\U00110000'}\n", false),
    ("translations = {'a': r'\\'}\n", false),
    ("translations = {'a': 'b'} 'junk'\n", false),
    ("translations = {'a':\u{a0}'b'}\n", false),
    ("translations = {'a': 'b'}\n\0", false),
    ("", false),
    ("# only a comment\n", false),
];

#[test]
fn synthetic_sources_match_python() {
    if !python_available() {
        return;
    }
    let dir = temp_dir("synthetic-dictionaries");
    let files: Vec<PathBuf> = SYNTHETIC
        .iter()
        .enumerate()
        .map(|(index, (source, _))| {
            let path = dir.join(format!("case{index:02}_dictionary.py"));
            fs::write(&path, source).expect("write case");
            path
        })
        .collect();
    let mut accepted = 0;
    for ((path, (source, must_parse)), expected) in
        files.iter().zip(SYNTHETIC).zip(python_results(&files))
    {
        let actual = parse_dictionary_literal(source);
        match (&actual, &expected) {
            (Ok(actual), Expected::Literal(python) | Expected::NotLiteral(python)) => {
                assert_eq!(actual, python, "{source:?} ({})", path.display());
                accepted += 1;
            }
            (Ok(_), Expected::Broken) => panic!("{source:?} parsed although Python fails"),
            (Err(error), _) => assert!(!must_parse, "{source:?} rejected: {error}"),
        }
    }
    eprintln!(
        "{} synthetic sources: {accepted} parsed identically, rest deferred",
        SYNTHETIC.len()
    );
}
