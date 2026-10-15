//! Breaking a sentence into lines, and cutting a line to its room — the
//! renderer's own rules, so a [`Flow`](crate::Flow) can lay a paragraph out
//! (and leave out the lines it scrolled away) before drawing it.

/// The words of `string` in lines no wider than `room`, greedily, as
/// `lxb-render`'s `Ui::wrap` breaks them: at whitespace; a word wider than
/// the room (an address, a long name) is broken between its letters, so no
/// line runs past the room.
pub fn wrap(string: &str, room: f32, mut measure: impl FnMut(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut words: Vec<String> = Vec::new();
    for word in string.split_whitespace() {
        if room > 0.0 && measure(word) > room {
            words.extend(broken(word, room, &mut measure));
        } else {
            words.push(word.to_owned());
        }
    }
    for word in &words {
        let word = word.as_str();
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && measure(&candidate) > room {
            lines.push(std::mem::take(&mut line));
            line = word.to_string();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// A word too wide for `room` in pieces that are not: as many letters as
/// fit in each (at least one).
fn broken(word: &str, room: f32, measure: &mut impl FnMut(&str) -> f32) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut piece = String::new();
    for letter in word.chars() {
        piece.push(letter);
        if measure(&piece) > room && piece.chars().count() > 1 {
            piece.pop();
            pieces.push(std::mem::take(&mut piece));
            piece.push(letter);
        }
    }
    if !piece.is_empty() {
        pieces.push(piece);
    }
    pieces
}

/// `string` as it fits in `room`: whole where it does, otherwise the longest
/// beginning of it that fits with an ellipsis after it.
pub fn fit(string: &str, room: f32, mut measure: impl FnMut(&str) -> f32) -> String {
    if room <= 0.0 {
        return String::new();
    }
    if measure(string) <= room {
        return string.to_string();
    }
    let ends: Vec<usize> = string.char_indices().map(|(at, _)| at).skip(1).collect();
    let (mut low, mut high) = (0usize, ends.len());
    // The longest prefix (by character count) that fits with the ellipsis.
    while low < high {
        let middle = (low + high).div_ceil(2);
        let candidate = format!("{}…", string[..ends[middle - 1]].trim_end());
        if measure(&candidate) <= room {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    if low == 0 {
        return "…".to_string();
    }
    format!("{}…", string[..ends[low - 1]].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monospace(text: &str) -> f32 {
        text.chars().count() as f32 * 10.0
    }

    #[test]
    fn words_are_carried_to_the_next_line_when_they_would_overflow() {
        assert_eq!(
            wrap("one two three four", 90.0, monospace),
            vec!["one two", "three", "four"]
        );
        assert_eq!(
            wrap("  spaced\n out ", 1000.0, monospace),
            vec!["spaced out"]
        );
        assert!(wrap("", 100.0, monospace).is_empty());
        assert_eq!(
            wrap("enormousword x", 30.0, monospace),
            vec!["eno", "rmo", "usw", "ord", "x"],
            "a word wider than the room is broken, not left to overflow"
        );
        assert_eq!(
            wrap("see https://example.org/a", 120.0, monospace),
            vec!["see", "https://exam", "ple.org/a"]
        );
    }

    #[test]
    fn a_line_too_long_is_cut_with_an_ellipsis() {
        assert_eq!(fit("Kernel", 60.0, monospace), "Kernel");
        assert_eq!(fit("Kernel version", 60.0, monospace), "Kerne…");
        assert_eq!(fit("Kernel version", 70.0, monospace), "Kernel…");
        assert_eq!(fit("Kernel", 5.0, monospace), "…");
        assert_eq!(fit("Kernel", 0.0, monospace), "");
        assert_eq!(fit("Zażółć gęślą", 50.0, monospace), "Zażó…");
    }
}
