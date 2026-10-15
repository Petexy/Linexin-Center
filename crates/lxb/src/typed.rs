//! What a keyboard typed into a field: letters, and Backspace (U+0008)
//! rubbing the last one out.
//!
//! For pictures taken without a keyboard, `LINEXIN_LXB_TYPED` scripts what
//! is typed: `SECONDS:TEXT` entries separated by `|`, `\b` in TEXT for a
//! Backspace. Each is typed once, into the first field taking text on or
//! after that second of the page's clock (tests only).

use std::sync::{Mutex, PoisonError};

/// The variable scripting what is typed in pictures.
pub const TYPED_VARIABLE: &str = "LINEXIN_LXB_TYPED";

const RUB: char = '\u{8}';

/// `text` with what was typed applied: letters added, a Backspace taking
/// the last letter off. Whether anything changed.
pub fn apply(text: &mut String, typed: &str) -> bool {
    let mut changed = false;
    for letter in typed.chars() {
        if letter == RUB {
            changed |= text.pop().is_some();
        } else if !letter.is_control() {
            text.push(letter);
            changed = true;
        }
    }
    changed
}

/// The scripted entries still to be typed, read once from the environment.
static SCRIPT: Mutex<Option<Vec<(f32, String)>>> = Mutex::new(None);

/// What the script types by `seconds` that was not typed yet.
pub(crate) fn scripted(seconds: f32) -> String {
    let mut script = SCRIPT.lock().unwrap_or_else(PoisonError::into_inner);
    let entries = script.get_or_insert_with(|| {
        std::env::var(TYPED_VARIABLE)
            .map(|value| parse(&value))
            .unwrap_or_default()
    });
    let mut text = String::new();
    entries.retain(|(at, typed)| {
        let due = *at <= seconds;
        if due {
            text.push_str(typed);
        }
        !due
    });
    text
}

fn parse(value: &str) -> Vec<(f32, String)> {
    value
        .split('|')
        .filter_map(|entry| {
            let (at, text) = entry.split_once(':')?;
            Some((
                at.trim().parse().ok()?,
                text.replace("\\b", &RUB.to_string()),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_are_added_and_rubbed_out() {
        let mut text = String::from("fire");
        assert!(apply(&mut text, "fox"));
        assert_eq!(text, "firefox");
        assert!(apply(&mut text, "\u{8}\u{8}\u{8}wall"));
        assert_eq!(text, "firewall");
        assert!(!apply(&mut String::new(), "\u{8}"));
        assert!(!apply(&mut text, "\n\t"));
        assert!(apply(&mut text, "żółw 🙂"));
        assert_eq!(text, "firewallżółw 🙂");
    }

    #[test]
    fn a_script_names_seconds_and_text() {
        assert_eq!(
            parse("0.5:fire|1:fox\\b|x|2.5:"),
            vec![
                (0.5, "fire".into()),
                (1.0, format!("fox{RUB}")),
                (2.5, String::new())
            ]
        );
    }
}
