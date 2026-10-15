//! Native reader for the plain-literal `*_dictionary.py` form. It accepts a
//! strict subset of Python and errs on anything it does not fully understand,
//! so an `Ok` result always equals what executing the file would produce.
//!
//! Besides `translations = {...}` the subset has the two statements the
//! widgets' dictionaries grew: a name bound to a string (`NAME = "..."`,
//! usable as a key or value afterwards) and `translations.update({...})`.

use std::collections::HashMap;

use crate::Dictionary;

pub(crate) fn parse(source: &str) -> Result<Dictionary, String> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    if source.contains('\0') {
        return Err("source contains a NUL character".into());
    }
    // Python reads source with universal newlines, also inside string literals.
    let source = source.replace("\r\n", "\n").replace('\r', "\n");
    check_declared_encoding(&source)?;
    let tokens = tokenize(&source)?;
    parse_tokens(&tokens)
}

/// PEP 263: a coding declaration in a comment on line 1 or 2 (line 2 only when
/// line 1 is blank or a comment). Only UTF-8 matches the caller's decoding.
fn check_declared_encoding(source: &str) -> Result<(), String> {
    for line in source.split('\n').take(2) {
        let content = line.trim_start_matches([' ', '\t', '\x0c']);
        let Some(comment) = content.strip_prefix('#') else {
            if content.is_empty() {
                continue;
            }
            break;
        };
        if let Some(encoding) = coding_spec(comment) {
            let normal = encoding.to_ascii_lowercase().replace('_', "-");
            if normal == "utf-8" || normal == "utf8" || normal.starts_with("utf-8-") {
                return Ok(());
            }
            return Err(format!("declared source encoding {encoding:?}"));
        }
    }
    Ok(())
}

/// Tokenizer's `get_coding_spec`: `coding[:=]`, blanks, then `[-\w.]+` (ASCII).
fn coding_spec(comment: &str) -> Option<&str> {
    let mut rest = comment;
    while let Some(at) = rest.find("coding") {
        rest = &rest[at + "coding".len()..];
        let Some(spec) = rest.strip_prefix([':', '=']) else {
            continue;
        };
        let spec = spec.trim_start_matches([' ', '\t']);
        let end = spec
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
            .unwrap_or(spec.len());
        if end > 0 {
            return Some(&spec[..end]);
        }
    }
    None
}

#[derive(Debug, PartialEq)]
enum Token {
    Name(String),
    Str(String),
    Op(char),
    /// End of a logical line outside brackets.
    Newline,
}

fn tokenize(source: &str) -> Result<Vec<(usize, Token)>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1;
    let mut depth = 0usize;
    // No token yet on the current logical line, and whether it is indented.
    let mut line_start = true;
    let mut indented = false;

    while let Some(&c) = chars.get(index) {
        match c {
            '\n' => {
                if depth == 0 {
                    if !line_start {
                        tokens.push((line, Token::Newline));
                    }
                    line_start = true;
                    indented = false;
                }
                line += 1;
                index += 1;
                continue;
            }
            ' ' | '\t' | '\x0c' => {
                indented |= line_start && depth == 0;
                index += 1;
                continue;
            }
            '#' => {
                while chars.get(index).is_some_and(|&c| c != '\n') {
                    index += 1;
                }
                continue;
            }
            _ => {}
        }
        if line_start && indented {
            return Err(format!("line {line}: unexpected indent"));
        }
        line_start = false;
        let token_line = line;
        let token = match c {
            '\'' | '"' => {
                let (value, end) = read_string(&chars, index, false, &mut line)?;
                index = end;
                Token::Str(value)
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = index;
                while chars
                    .get(index)
                    .is_some_and(|&c| c.is_ascii_alphanumeric() || c == '_')
                {
                    index += 1;
                }
                let word: String = chars[start..index].iter().collect();
                if matches!(chars.get(index), Some('\'' | '"')) {
                    let raw = match word.as_str() {
                        "r" | "R" => true,
                        "u" | "U" => false,
                        _ => {
                            return Err(format!("line {line}: unsupported string prefix {word:?}"));
                        }
                    };
                    let (value, end) = read_string(&chars, index, raw, &mut line)?;
                    index = end;
                    Token::Str(value)
                } else {
                    Token::Name(word)
                }
            }
            '{' | '}' | '(' | ')' | '=' | ':' | ',' | '.' => {
                if c == '{' || c == '(' {
                    depth += 1;
                } else if c == '}' || c == ')' {
                    depth = depth
                        .checked_sub(1)
                        .ok_or(format!("line {line}: unmatched {c:?}"))?;
                }
                index += 1;
                Token::Op(c)
            }
            _ => return Err(format!("line {line}: unsupported character {c:?}")),
        };
        tokens.push((token_line, token));
    }
    if depth != 0 {
        return Err("unclosed bracket".into());
    }
    Ok(tokens)
}

/// Reads the literal whose opening quote is at `start`; returns its value and
/// the index after the closing quote.
fn read_string(
    chars: &[char],
    start: usize,
    raw: bool,
    line: &mut usize,
) -> Result<(String, usize), String> {
    let quote = chars[start];
    let triple = chars.get(start + 1) == Some(&quote) && chars.get(start + 2) == Some(&quote);
    let mut index = start + if triple { 3 } else { 1 };
    let mut value = String::new();
    loop {
        let Some(&c) = chars.get(index) else {
            return Err(format!("line {line}: unterminated string"));
        };
        index += 1;
        match c {
            c if c == quote => {
                if !triple {
                    return Ok((value, index));
                }
                if chars.get(index) == Some(&quote) && chars.get(index + 1) == Some(&quote) {
                    return Ok((value, index + 2));
                }
                value.push(c);
            }
            '\n' => {
                if !triple {
                    return Err(format!("line {line}: unterminated string"));
                }
                *line += 1;
                value.push(c);
            }
            '\\' => {
                let Some(&next) = chars.get(index) else {
                    return Err(format!("line {line}: unterminated string"));
                };
                index += 1;
                if next == '\n' {
                    *line += 1;
                }
                if raw {
                    value.push('\\');
                    value.push(next);
                    continue;
                }
                match next {
                    '\n' => {}
                    '\\' | '\'' | '"' => value.push(next),
                    'a' => value.push('\x07'),
                    'b' => value.push('\x08'),
                    'f' => value.push('\x0c'),
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    'v' => value.push('\x0b'),
                    '0'..='7' => {
                        let mut code = next.to_digit(8).unwrap_or_default();
                        for _ in 0..2 {
                            match chars.get(index).and_then(|c| c.to_digit(8)) {
                                Some(digit) => {
                                    code = code * 8 + digit;
                                    index += 1;
                                }
                                None => break,
                            }
                        }
                        value.push(char::from_u32(code).ok_or("invalid octal escape")?);
                    }
                    'x' | 'u' | 'U' => {
                        let width = match next {
                            'x' => 2,
                            'u' => 4,
                            _ => 8,
                        };
                        let digits: String = chars.iter().skip(index).take(width).collect();
                        if digits.len() != width || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
                            return Err(format!("line {line}: truncated \\{next} escape"));
                        }
                        index += width;
                        let code = u32::from_str_radix(&digits, 16).map_err(|e| e.to_string())?;
                        // Lone surrogates are valid in Python but not in Rust.
                        let decoded = char::from_u32(code).ok_or(format!(
                            "line {line}: unrepresentable escape \\{next}{digits}"
                        ))?;
                        value.push(decoded);
                    }
                    // Named escapes need Python's Unicode name database.
                    'N' => return Err(format!("line {line}: \\N{{...}} escape")),
                    // Unknown escapes keep the backslash (Python only warns).
                    _ => {
                        value.push('\\');
                        value.push(next);
                    }
                }
            }
            _ => value.push(c),
        }
    }
}

/// `[docstring NEWLINE] translations = { [str: str {, str: str} [,]] } [NEWLINE]`
/// where `str` is one or more adjacent string literals.
fn parse_tokens(tokens: &[(usize, Token)]) -> Result<Dictionary, String> {
    let mut cursor = Cursor {
        tokens,
        position: 0,
    };
    if cursor.string().is_some() {
        cursor.expect(&Token::Newline)?;
    }
    // Names bound to strings so far, and the dictionary once assigned.
    let mut names: HashMap<String, String> = HashMap::new();
    let mut translations: Option<Entries> = None;
    let translations_name = Token::Name("translations".into());
    loop {
        match cursor.tokens.get(cursor.position) {
            None => break,
            Some((_, token)) if *token == translations_name => {
                cursor.position += 1;
                if cursor.accept(&Token::Op('=')) {
                    if translations.is_some() {
                        return Err(cursor.error("translations assigned again"));
                    }
                    let mut entries = Entries::default();
                    for (key, value) in cursor.dict(&names)? {
                        entries.insert(key, value);
                    }
                    translations = Some(entries);
                } else {
                    cursor.expect(&Token::Op('.'))?;
                    cursor.expect(&Token::Name("update".into()))?;
                    cursor.expect(&Token::Op('('))?;
                    let pairs = cursor.dict(&names)?;
                    cursor.expect(&Token::Op(')'))?;
                    let entries = translations.as_mut().ok_or_else(|| {
                        cursor.error("translations updated before it is assigned")
                    })?;
                    for (key, value) in pairs {
                        entries.insert(key, value);
                    }
                }
            }
            Some((_, Token::Name(name))) => {
                let name = name.clone();
                cursor.position += 1;
                cursor.expect(&Token::Op('='))?;
                let value = cursor
                    .string()
                    .ok_or_else(|| cursor.error("expected a string"))?;
                names.insert(name, value);
            }
            Some(_) => return Err(cursor.error("unexpected statement")),
        }
        if cursor.position < cursor.tokens.len() {
            cursor.expect(&Token::Newline)?;
        }
    }
    translations
        .map(|entries| entries.entries)
        .ok_or_else(|| "no translations dictionary".into())
}

/// A dictionary being built: Python dicts keep the first position and the
/// last value of a key, and `update` does the same.
#[derive(Default)]
struct Entries {
    entries: Dictionary,
    positions: HashMap<String, usize>,
}

impl Entries {
    fn insert(&mut self, key: String, value: String) {
        match self.positions.get(&key) {
            Some(&position) => self.entries[position].1 = value,
            None => {
                self.positions.insert(key.clone(), self.entries.len());
                self.entries.push((key, value));
            }
        }
    }
}

struct Cursor<'a> {
    tokens: &'a [(usize, Token)],
    position: usize,
}

impl Cursor<'_> {
    fn accept(&mut self, token: &Token) -> bool {
        let matched = self
            .tokens
            .get(self.position)
            .is_some_and(|(_, next)| next == token);
        self.position += usize::from(matched);
        matched
    }

    fn expect(&mut self, token: &Token) -> Result<(), String> {
        if self.accept(token) {
            Ok(())
        } else {
            Err(self.error(&format!("expected {token:?}")))
        }
    }

    /// Adjacent string literals, concatenated.
    fn string(&mut self) -> Option<String> {
        let mut value: Option<String> = None;
        while let Some((_, Token::Str(part))) = self.tokens.get(self.position) {
            value.get_or_insert_default().push_str(part);
            self.position += 1;
        }
        value
    }

    /// A string literal, or a name bound to one.
    fn term(&mut self, names: &HashMap<String, String>) -> Option<String> {
        if let Some(value) = self.string() {
            return Some(value);
        }
        match self.tokens.get(self.position) {
            Some((_, Token::Name(name))) => {
                let value = names.get(name)?.clone();
                self.position += 1;
                Some(value)
            }
            _ => None,
        }
    }

    /// `{ key: value, ... }` with string (or bound name) keys and values, in
    /// order, duplicates included.
    fn dict(&mut self, names: &HashMap<String, String>) -> Result<Vec<(String, String)>, String> {
        self.expect(&Token::Op('{'))?;
        let mut pairs = Vec::new();
        while !self.accept(&Token::Op('}')) {
            let key = self
                .term(names)
                .ok_or_else(|| self.error("expected a string key"))?;
            self.expect(&Token::Op(':'))?;
            let value = self
                .term(names)
                .ok_or_else(|| self.error("expected a string value"))?;
            pairs.push((key, value));
            if !self.accept(&Token::Op(',')) {
                self.expect(&Token::Op('}'))?;
                break;
            }
        }
        Ok(pairs)
    }

    fn error(&self, message: &str) -> String {
        match self.tokens.get(self.position) {
            Some((line, token)) => format!("line {line}: {message}, found {token:?}"),
            None => format!("{message}, found end of file"),
        }
    }
}
