//! Runtime translations for Linexin Center — a port of the original Python
//! `WidgetLocalizationManager` and its `_translate_*` helpers.
//!
//! # Dictionaries
//!
//! Every widget (legacy Python or native `.so`) and the Center itself
//! (domain [`HOST_DOMAIN`]) read
//! `<widgets dir>/localization/<lang>/<domain>_dictionary.py`, e.g.
//! `/usr/share/linexin/widgets/localization/pl_PL/system_information_dictionary.py`:
//!
//! ```python
//! translations = {
//!     "System Information": "Informacje o systemie",
//!     "Downloading {}...": "Pobieranie {}...",
//! }
//! ```
//!
//! `<lang>` is a locale such as `de_DE`, `pl_PL` or `zh_CN`; `<domain>` is the
//! file name without `_dictionary.py` (a native widget's `widget_id`). Keep the
//! file a plain literal (comments, a docstring and implicit string
//! concatenation are fine): it is then read natively. Anything else (variables,
//! `translations.update(...)`) is executed by Python like the original did,
//! which needs the Python runtime.
//!
//! # Lookup
//!
//! The active language is `--lang`, else the system locale (see
//! [`detect_language`]). A string is looked up as an exact key in the active
//! language's dictionary, then in [`FALLBACK_LANGUAGE`]'s, else returned as is.
//! [`Localization::translate`] (what widgets' `_()` does) additionally treats
//! keys containing `{}` as patterns: `"Downloading {}..."` turns
//! `"Downloading Firefox..."` into `"Pobieranie Firefox..."`, translating the
//! captured part too. Patterns come from the active language's dictionary, or
//! from en_US's when the active one is missing or empty.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

use regex::Regex;

mod literal;
mod locale;
mod locale_alias;
pub mod widgets;

/// Language used when the active language lacks a dictionary or key.
pub const FALLBACK_LANGUAGE: &str = "en_US";
/// Dictionary domain of the Center's own strings (original `widget_name="widget_loader"`).
pub const HOST_DOMAIN: &str = "widget_loader";

/// Nesting depth of `{}` patterns at which the original's recursion raised
/// `RecursionError` (Python's default recursion limit).
const MAX_NESTED_PATTERNS: usize = 1000;

/// One `translations` mapping as `(key, value)` pairs in Python dict iteration
/// order: a repeated key keeps its first position and takes the last value.
/// Order matters because `{}` patterns are tried in this order.
pub type Dictionary = Vec<(String, String)>;

/// Evaluates a dictionary file that is not a plain literal (e.g. Alexy AI's,
/// which uses variables and `translations.update(...)`) by executing it, as the
/// original Center did, and returns its final `translations` dict.
pub type Evaluator = dyn Fn(&Path) -> Result<Dictionary, String> + Send + Sync;

/// Active language, mirroring the original `_get_system_language`:
/// a non-empty `override_language` (from `--lang`) verbatim; otherwise the
/// language of `locale.getlocale()` in a `python3` started with this
/// environment: the LC_CTYPE locale from LC_ALL, LC_CTYPE, LANG if installed,
/// PEP 538 coerced to `C.UTF-8` when that yields the C locale, normalized with
/// Python's `locale_alias` (so `pl_PL.UTF-8` → `pl_PL`, a missing locale or `C`
/// → `C`); otherwise (`LC_ALL=C`, invalid LC_ALL, unknown names) `LANG` up to
/// its first `.`; otherwise [`FALLBACK_LANGUAGE`].
pub fn detect_language(override_language: Option<&str>) -> String {
    if let Some(language) = override_language.filter(|language| !language.is_empty()) {
        return language.to_owned();
    }
    if let Ok(Some(language)) = locale::python_getlocale_language()
        && !language.is_empty()
    {
        return language;
    }
    let lang = std::env::var_os("LANG").map(|value| value.to_string_lossy().into_owned());
    match lang.as_deref().and_then(|lang| lang.split('.').next()) {
        Some(code) if !code.is_empty() => code.to_owned(),
        _ => FALLBACK_LANGUAGE.to_owned(),
    }
}

/// Parse the plain-literal dictionary form without executing anything:
/// optional comments/blank lines/docstring, then `translations = { ... }` whose
/// keys and values are Python string literals (single/double/triple quotes,
/// r/u prefixes, Python escapes except `\N{...}`, implicit concatenation,
/// trailing comma). Anything else returns `Err` so the caller can fall back to
/// an [`Evaluator`]; an `Ok` result always equals what Python would produce.
pub fn parse_dictionary_literal(source: &str) -> Result<Dictionary, String> {
    literal::parse(source)
}

/// Thread-safe translation catalog (Python worker threads call into it).
/// Immutable after [`Localization::load`].
pub struct Localization {
    language: String,
    /// Every domain with a dictionary file in any language.
    domains: HashSet<String>,
    catalogs: HashMap<String, DomainCatalogs>,
    diagnostics: Vec<String>,
}

struct DomainCatalogs {
    active: Option<Catalog>,
    /// en_US, unless that is the active language.
    fallback: Option<Catalog>,
}

struct Catalog {
    exact: HashMap<String, String>,
    /// Python truthiness of the dict, which decides whose patterns are used.
    non_empty: bool,
    /// `{}` keys in dictionary order, except those translated to themselves
    /// (the original skipped them after matching).
    patterns: Vec<Pattern>,
}

struct Pattern {
    regex: Regex,
    translation: String,
}

impl Catalog {
    fn new(dictionary: Dictionary, path: &Path, diagnostics: &mut Vec<String>) -> Self {
        let non_empty = !dictionary.is_empty();
        let mut patterns = Vec::new();
        for (key, value) in &dictionary {
            if !key.contains("{}") || key == value {
                continue;
            }
            // Python: re.escape(key) with the escaped "{}" replaced by "(.*)".
            let source: Vec<String> = key.split("{}").map(regex::escape).collect();
            match Regex::new(&source.join("(.*)")) {
                Ok(regex) => patterns.push(Pattern {
                    regex,
                    translation: value.clone(),
                }),
                Err(error) => {
                    diagnostics.push(format!("{}: pattern {key:?}: {error}", path.display()))
                }
            }
        }
        Self {
            exact: dictionary.into_iter().collect(),
            non_empty,
            patterns,
        }
    }
}

impl Localization {
    /// Index every `<base_dir>/<lang>/<domain>_dictionary.py` of each base dir
    /// (normally `<widgets dir>/localization`; earlier dirs win for the same
    /// language and domain), then load the dictionaries of `language` and
    /// [`FALLBACK_LANGUAGE`] eagerly. Literal files are parsed natively; others go
    /// through `evaluator` (skipped with a diagnostic when `None`). Missing dirs
    /// are ignored; unreadable/invalid files become diagnostics.
    pub fn load(base_dirs: &[PathBuf], language: &str, evaluator: Option<&Evaluator>) -> Self {
        let mut diagnostics = Vec::new();
        let index = index_dictionaries(base_dirs, &mut diagnostics);
        let mut load = |files: &HashMap<String, PathBuf>, language: &str| {
            let path = files.get(language)?;
            match read_dictionary(path, evaluator) {
                Ok(dictionary) => Some(Catalog::new(dictionary, path, &mut diagnostics)),
                Err(error) => {
                    diagnostics.push(error);
                    None
                }
            }
        };
        let mut catalogs = HashMap::new();
        for (domain, files) in &index {
            let active = load(files, language);
            let fallback = if language == FALLBACK_LANGUAGE {
                None
            } else {
                load(files, FALLBACK_LANGUAGE)
            };
            if active.is_some() || fallback.is_some() {
                catalogs.insert(domain.clone(), DomainCatalogs { active, fallback });
            }
        }
        Self {
            language: language.to_owned(),
            domains: index.into_keys().collect(),
            catalogs,
            diagnostics,
        }
    }

    /// Catalog without dictionaries (every lookup returns its input).
    pub fn empty(language: &str) -> Self {
        Self {
            language: language.to_owned(),
            domains: HashSet::new(),
            catalogs: HashMap::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    /// Problems found while indexing/loading, for stderr.
    pub fn diagnostics(&self) -> Vec<String> {
        self.diagnostics.clone()
    }

    /// Whether any language directory has a dictionary for `domain`
    /// (original `domain in manager.widget_translations`).
    pub fn has_domain(&self, domain: &str) -> bool {
        self.domains.contains(domain)
    }

    /// Original `WidgetLocalizationManager.get_text`: exact key in the active
    /// language, then in [`FALLBACK_LANGUAGE`], else `key`.
    pub fn get_text(&self, key: &str, domain: &str) -> String {
        self.lookup(key, domain).unwrap_or(key).to_owned()
    }

    fn lookup(&self, key: &str, domain: &str) -> Option<&str> {
        let catalogs = self.catalogs.get(domain)?;
        [&catalogs.active, &catalogs.fallback]
            .into_iter()
            .flatten()
            .find_map(|catalog| catalog.exact.get(key))
            .map(String::as_str)
    }

    /// Original `widget_trans.get(lang) or widget_trans.get(FALLBACK_LANGUAGE, {})`.
    fn patterns(&self, domain: &str) -> &[Pattern] {
        let Some(catalogs) = self.catalogs.get(domain) else {
            return &[];
        };
        match (&catalogs.active, &catalogs.fallback) {
            (Some(active), _) if active.non_empty => &active.patterns,
            (_, Some(fallback)) => &fallback.patterns,
            _ => &[],
        }
    }

    /// Original `_translate_with_patterns_helper`: [`Self::get_text`]; if that
    /// returns the input unchanged, try each `{}`-containing key of the active
    /// dictionary (or en_US when the active language has none) as a regex
    /// (escaped, `{}` → `(.*)`), searching `text`; on the first match whose
    /// translation differs, recursively translate the captured group, substitute
    /// it for the first `{}` of the translation, and replace every occurrence of
    /// the matched text in `text`. Empty input returns empty. Where the
    /// original's recursion overflowed (nesting deeper than Python's recursion
    /// limit, e.g. a key that is just `{}`), `text` is returned unchanged.
    pub fn translate(&self, text: &str, domain: &str) -> String {
        // The recursion only nests along one path, so it runs as a loop: descend
        // into captured groups, then substitute on the way back up.
        struct Level<'a> {
            text: String,
            matched: String,
            translation: &'a str,
        }
        let patterns = self.patterns(domain);
        let mut levels: Vec<Level> = Vec::new();
        let mut current = text.to_owned();
        let mut result = loop {
            if current.is_empty() {
                break String::new();
            }
            if let Some(translated) = self.lookup(&current, domain)
                && translated != current
            {
                break translated.to_owned();
            }
            let found = patterns.iter().find_map(|pattern| {
                let captures = pattern.regex.captures(&current)?;
                let (matched, content) = (captures.get(0)?, captures.get(1)?);
                Some((
                    matched.as_str().to_owned(),
                    content.as_str().to_owned(),
                    pattern,
                ))
            });
            let Some((matched, content, pattern)) = found else {
                break current;
            };
            if levels.len() == MAX_NESTED_PATTERNS {
                return text.to_owned();
            }
            levels.push(Level {
                text: current,
                matched,
                translation: &pattern.translation,
            });
            current = content;
        };
        while let Some(level) = levels.pop() {
            let translated = level.translation.replacen("{}", &result, 1);
            result = level.text.replace(&level.matched, &translated);
        }
        result
    }

    /// Original `_translate_markup_content_static`: [`Self::translate`] the
    /// whole markup, then translate (with [`Self::get_text`]) the stripped text
    /// between each `>` and `<`, keeping its surrounding whitespace.
    pub fn translate_markup(&self, markup: &str, domain: &str) -> String {
        static TEXT_BETWEEN_TAGS: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(">([^<]+)<").expect("constant regex"));
        let markup = self.translate(markup, domain);
        TEXT_BETWEEN_TAGS
            .replace_all(&markup, |captures: &regex::Captures| {
                let content = &captures[1];
                let stripped = content.trim_matches(is_python_space);
                match self.lookup(stripped, domain) {
                    Some(translated) if !stripped.is_empty() && translated != stripped => {
                        let leading = &content
                            [..content.len() - content.trim_start_matches(is_python_space).len()];
                        let trailing = &content[content.trim_end_matches(is_python_space).len()..];
                        format!(">{leading}{translated}{trailing}<")
                    }
                    _ => captures[0].to_owned(),
                }
            })
            .into_owned()
    }

    /// Original `_prepare_module` key selection for a widget file stem such as
    /// `z-system-information-widget` or `a-system_updater`: candidates are the
    /// stem, the stem without a one-letter `x-` prefix, then underscore variants
    /// of both; the first candidate with [`Self::has_domain`] wins; otherwise the
    /// first candidate whose `_widget`/`-widget` removal has a domain; otherwise
    /// the stem itself.
    pub fn domain_for_module(&self, filename_stem: &str) -> String {
        let mut candidates = vec![filename_stem.to_owned()];
        let mut chars = filename_stem.chars();
        if filename_stem.chars().count() > 2 && chars.nth(1) == Some('-') {
            candidates.push(chars.collect());
        }
        let underscored: Vec<String> = candidates.iter().map(|key| key.replace('-', "_")).collect();
        candidates.extend(underscored);
        if let Some(key) = candidates.iter().find(|key| self.has_domain(key)) {
            return key.clone();
        }
        candidates
            .iter()
            .map(|key| key.replace("_widget", "").replace("-widget", ""))
            .find(|key| self.has_domain(key))
            .unwrap_or_else(|| filename_stem.to_owned())
    }
}

/// Python's `str.isspace()`, which `str.strip()` uses: Unicode White_Space plus
/// the ASCII information separators.
fn is_python_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// domain → language → file, mirroring the original `_load_all_translations`
/// walk (`<lang dir>.glob("*_dictionary.py")`, domain = stem without every
/// `_dictionary`).
fn index_dictionaries(
    base_dirs: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> HashMap<String, HashMap<String, PathBuf>> {
    let mut index: HashMap<String, HashMap<String, PathBuf>> = HashMap::new();
    for base_dir in base_dirs {
        let language_dirs = match sorted_entries(base_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                diagnostics.push(format!("{}: {error}", base_dir.display()));
                continue;
            }
        };
        for (language, language_dir) in language_dirs {
            if !language_dir.is_dir() {
                continue;
            }
            let files = match sorted_entries(&language_dir) {
                Ok(files) => files,
                Err(error) => {
                    diagnostics.push(format!("{}: {error}", language_dir.display()));
                    continue;
                }
            };
            for (name, path) in files {
                let Some(stem) = name.strip_suffix(".py") else {
                    continue;
                };
                if !stem.ends_with("_dictionary") || !path.is_file() {
                    continue;
                }
                let domain = stem.replace("_dictionary", "");
                index
                    .entry(domain)
                    .or_default()
                    .entry(language.clone())
                    .or_insert(path);
            }
        }
    }
    index
}

/// UTF-8 named entries of `dir` sorted by name.
fn sorted_entries(dir: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if let Ok(name) = entry.file_name().into_string() {
            entries.push((name, entry.path()));
        }
    }
    entries.sort();
    Ok(entries)
}

fn read_dictionary(path: &Path, evaluator: Option<&Evaluator>) -> Result<Dictionary, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if let Ok(source) = std::str::from_utf8(&bytes)
        && let Ok(dictionary) = parse_dictionary_literal(source)
    {
        return Ok(dictionary);
    }
    let evaluate = evaluator.ok_or_else(|| {
        format!(
            "{}: not a plain literal dictionary and Python is unavailable",
            path.display()
        )
    })?;
    evaluate(path).map_err(|error| format!("{}: {error}", path.display()))
}

static GLOBAL: OnceLock<Localization> = OnceLock::new();

/// Install the process-wide catalog (first call wins). Must happen before the
/// UI or any widget is created.
pub fn install(localization: Localization) {
    let _ = GLOBAL.set(localization);
}

/// Process-wide catalog; an empty en_US catalog if none was installed.
pub fn global() -> &'static Localization {
    GLOBAL.get_or_init(|| Localization::empty(FALLBACK_LANGUAGE))
}

/// The Center's own `_()`: `global().get_text(text, HOST_DOMAIN)`.
pub fn tr(text: &str) -> String {
    global().get_text(text, HOST_DOMAIN)
}

#[cfg(test)]
mod tests;
