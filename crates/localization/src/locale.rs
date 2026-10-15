//! What `locale.getlocale()[0]` reports in a `python3` started with the current
//! environment, without running Python: the LC_CTYPE locale CPython sets at
//! startup (including PEP 538 C-locale coercion), parsed like `locale.py` does.

use std::ffi::CStr;

use crate::locale_alias::LOCALE_ALIAS;

/// The language code, `Ok(None)` for Python's `(None, …)`, `Err(())` where
/// Python raises `ValueError: unknown locale`.
pub(crate) fn python_getlocale_language() -> Result<Option<String>, ()> {
    parse_localename_language(&python_ctype_locale())
}

/// `setlocale(LC_CTYPE, NULL)` after CPython's start-up.
fn python_ctype_locale() -> String {
    // setlocale(LC_CTYPE, "") failing leaves the initial "C" locale in place.
    let current = ctype_locale_name(c"").unwrap_or_else(|| "C".to_owned());
    if current == "C" && python_coerces_c_locale() {
        // _TARGET_LOCALES of Python/pylifecycle.c, in order.
        for target in [c"C.UTF-8", c"C.utf8", c"UTF-8"] {
            if let Some(coerced) = ctype_locale_name(target) {
                return coerced;
            }
        }
    }
    current
}

/// PEP 538: only when LC_ALL is unset/empty and PYTHONCOERCECLOCALE is not "0".
fn python_coerces_c_locale() -> bool {
    let lc_all_set = std::env::var_os("LC_ALL").is_some_and(|value| !value.is_empty());
    let disabled = std::env::var_os("PYTHONCOERCECLOCALE").is_some_and(|value| value == "0");
    !lc_all_set && !disabled
}

/// Name `setlocale(LC_CTYPE, name)` would report afterwards, or `None` when it
/// would fail. `""` resolves LC_ALL, LC_CTYPE, LANG like setlocale does.
#[cfg(target_env = "gnu")]
fn ctype_locale_name(name: &CStr) -> Option<String> {
    // glibc's _NL_LOCALE_NAME(LC_CTYPE): _NL_ITEM(LC_CTYPE, _NL_ITEM_INDEX(-1)).
    const NL_LOCALE_NAME_CTYPE: libc::nl_item = (libc::LC_CTYPE << 16) | 0xffff;
    // SAFETY: `name` is NUL-terminated. The returned locale object is only used
    // here; its name is copied before the object is freed.
    unsafe {
        let locale = libc::newlocale(libc::LC_CTYPE_MASK, name.as_ptr(), std::ptr::null_mut());
        if locale.is_null() {
            return None;
        }
        let reported = libc::nl_langinfo_l(NL_LOCALE_NAME_CTYPE, locale);
        let result =
            (!reported.is_null()).then(|| CStr::from_ptr(reported).to_string_lossy().into_owned());
        libc::freelocale(locale);
        result
    }
}

#[cfg(not(target_env = "gnu"))]
fn ctype_locale_name(name: &CStr) -> Option<String> {
    let requested = if name.is_empty() {
        ["LC_ALL", "LC_CTYPE", "LANG"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok())
            .find(|value| !value.is_empty())
            .unwrap_or_else(|| "C".to_owned())
    } else {
        name.to_string_lossy().into_owned()
    };
    let c_name = std::ffi::CString::new(requested.as_str()).ok()?;
    // SAFETY: `c_name` is NUL-terminated; the locale object is freed right away.
    unsafe {
        let locale = libc::newlocale(libc::LC_CTYPE_MASK, c_name.as_ptr(), std::ptr::null_mut());
        if locale.is_null() {
            return None;
        }
        libc::freelocale(locale);
    }
    Some(if requested == "POSIX" {
        "C".to_owned()
    } else {
        requested
    })
}

/// Language part of `locale._parse_localename(localename)`.
fn parse_localename_language(localename: &str) -> Result<Option<String>, ()> {
    let normalized = normalize(localename);
    let (code, modifier) = match normalized.split_once('@') {
        Some((code, modifier)) => (code, Some(modifier)),
        None => (normalized.as_str(), None),
    };
    if modifier == Some("euro") && !code.contains('.') {
        return Ok(Some(code.to_owned()));
    }
    if let Some((language, _)) = code.split_once('.') {
        return Ok(Some(language.to_owned()));
    }
    if code == "C" || code == "UTF-8" {
        return Ok(None);
    }
    Err(())
}

fn alias(name: &str) -> Option<&'static str> {
    LOCALE_ALIAS
        .binary_search_by(|(key, _)| (*key).cmp(name))
        .ok()
        .map(|index| LOCALE_ALIAS[index].1)
}

/// Port of `locale.normalize`. Encodings are not canonicalized (that needs
/// Python's codec tables); every caller only reads the language part, which is
/// reproduced exactly.
fn normalize(localename: &str) -> String {
    let lowered = localename.to_lowercase().replace(':', ".");
    let (code, modifier) = lowered.split_once('@').unwrap_or((lowered.as_str(), ""));
    let mut parts = code.split('.');
    let langname = parts.next().unwrap_or_default();
    let encoding = parts.next().unwrap_or_default();

    let mut lang_enc = langname.to_owned();
    if !encoding.is_empty() {
        lang_enc.push('.');
        lang_enc.push_str(&encoding.replace(['-', '_'], ""));
    }
    let with_modifier = |name: &str| {
        if modifier.is_empty() {
            name.to_owned()
        } else {
            format!("{name}@{modifier}")
        }
    };

    if let Some(code) = alias(&with_modifier(&lang_enc)) {
        return code.to_owned();
    }
    if !modifier.is_empty()
        && let Some(code) = alias(&lang_enc)
    {
        match code.split_once('@') {
            None => return append_modifier(code, modifier),
            Some((_, default)) if default.to_lowercase() == modifier => return code.to_owned(),
            Some(_) => {}
        }
    }
    if !encoding.is_empty() {
        if let Some(code) = alias(&with_modifier(langname)) {
            return match code.split_once('@') {
                None => replace_encoding(code, encoding),
                Some((code, modifier)) => {
                    format!("{}@{modifier}", replace_encoding(code, encoding))
                }
            };
        }
        if !modifier.is_empty()
            && let Some(code) = alias(langname)
        {
            match code.split_once('@') {
                None => return append_modifier(&replace_encoding(code, encoding), modifier),
                Some((code, default)) if default.to_lowercase() == modifier => {
                    return format!("{}@{default}", replace_encoding(code, encoding));
                }
                Some(_) => {}
            }
        }
    }
    localename.to_owned()
}

fn replace_encoding(code: &str, encoding: &str) -> String {
    let langname = code.split_once('.').map_or(code, |(langname, _)| langname);
    format!("{langname}.{encoding}")
}

/// `_append_modifier`; its re-encoding of `…@euro` codes keeps the language.
fn append_modifier(code: &str, modifier: &str) -> String {
    if modifier == "euro" && !code.contains('.') {
        format!("{code}.ISO8859-15")
    } else {
        format!("{code}@{modifier}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::python;

    fn hex(text: &str) -> String {
        text.bytes().map(|byte| format!("{byte:02x}")).collect()
    }

    fn unhex(text: &str) -> String {
        let bytes = (0..text.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
            .collect();
        String::from_utf8(bytes).unwrap()
    }

    fn name_corpus() -> Vec<String> {
        let mut names = vec![
            String::new(),
            "C".into(),
            "POSIX".into(),
            "C.UTF-8".into(),
            "C.utf8".into(),
            "UTF-8".into(),
            "utf8".into(),
            ".UTF-8".into(),
            "pl_PL.".into(),
            "@euro".into(),
            "xx_YY".into(),
            "xx_YY.UTF-8".into(),
            "xx_YY@euro".into(),
            "xx_YY.UTF-8@euro".into(),
            "a.b.c".into(),
            "de_DE:UTF-8".into(),
            "de_DE.ISO8859-1@euro".into(),
            "de_DE.UTF-8@EURO".into(),
            "sr_RS.UTF-8@latin".into(),
            "sr_RS@Latin".into(),
            "ca_ES@valencia".into(),
            "ca_ES.UTF-8@valencia".into(),
            "be_BY@latin".into(),
            "uz_UZ@cyrillic".into(),
            "tt_RU@iqtelif".into(),
            "en_US.UTF-8@foo".into(),
            "/usr/lib/locale/pl_PL.utf8".into(),
            "pl_PL.UTF-8;LC_CTYPE=en_US".into(),
            "İstanbul".into(),
            "ÉTÉ.utf8".into(),
        ];
        let suffixes = [
            "",
            ".UTF-8",
            ".utf8",
            ".utf-8",
            ".ISO-8859-1",
            ".ISO8859-15",
            ".iso88592",
            ".GB18030",
            ".KOI8-R",
            ".eucJP",
            "@euro",
            "@latin",
            "@cyrillic",
            "@valencia",
            "@devanagari",
            ".UTF-8@euro",
            ".ISO8859-1@euro",
            ".UTF-8@latin",
            ".UTF-8@foo",
        ];
        for (key, value) in LOCALE_ALIAS {
            for base in [key.to_string(), value.to_string(), key.to_uppercase()] {
                let stem = base.split(['.', '@']).next().unwrap_or_default().to_owned();
                names.push(base);
                names.extend(suffixes.iter().map(|suffix| format!("{stem}{suffix}")));
            }
        }
        names.sort();
        names.dedup();
        names
    }

    #[test]
    fn normalization_matches_python_parse_localename() {
        let names = name_corpus();
        let script = "import locale, sys\n\
            for line in sys.stdin:\n\
            \x20   name = bytes.fromhex(line.strip()).decode()\n\
            \x20   try:\n\
            \x20       language = locale._parse_localename(name)[0]\n\
            \x20   except ValueError:\n\
            \x20       print('E')\n\
            \x20   else:\n\
            \x20       print('N' if language is None else 'S' + language.encode().hex())\n";
        let input: String = names.iter().map(|name| hex(name) + "\n").collect();
        let Some(output) = python(&["-c", script], &input) else {
            return;
        };
        let expected: Vec<Result<Option<String>, ()>> = output
            .lines()
            .map(|line| match line {
                "E" => Err(()),
                "N" => Ok(None),
                _ => Ok(Some(unhex(&line[1..]))),
            })
            .collect();
        assert_eq!(expected.len(), names.len());
        let mismatches: Vec<_> = names
            .iter()
            .zip(&expected)
            .filter(|(name, expected)| parse_localename_language(name) != **expected)
            .map(|(name, expected)| (name, expected, parse_localename_language(name)))
            .collect();
        assert!(
            mismatches.is_empty(),
            "{} mismatches: {mismatches:?}",
            mismatches.len()
        );
        eprintln!(
            "{} locale names normalized identically to Python",
            names.len()
        );
    }

    #[test]
    fn alias_table_is_sorted() {
        assert!(LOCALE_ALIAS.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }
}
