//! What the buttons do, drawn rather than spelled out: the row LineXinBar
//! writes in the corner of its start screen, and DistriBumpy along the foot
//! of every page — a word and a picture of the button that does it, laid out
//! from the right-hand margin leftwards.
//!
//! The same act is South on a pad and Enter on a keyboard, so the picture
//! follows what the hands are on ([`Hands`]). Every pair is also a target: a
//! click on one is a press of the button it pictures ([`Legend::pressed`]),
//! which is how a mouse gets back out of a page whose Back is only drawn
//! here. Nothing about moving: the arrows are the one thing a page does not
//! have to explain.

use crate::page::Page;
use crate::types::{Action, Align, Role, Text};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Where a [`Legend`] numbers its pairs' pointer spots from.
pub const LEGEND_SPOTS: u32 = 0x4C5B_0000;

/// A picture of a button rather larger than the word beside it, because the
/// picture is the half being read (the shell's proportions, against the
/// 1080-pixel reference).
const GLYPH: f32 = 28.0;
const GAP: f32 = 7.0;
const STEP: f32 = 22.0;

/// The buttons a legend names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// Acts on what the light is on.
    Accept,
    /// Raises the other answers (a context menu).
    Options,
    /// Gets back out.
    Back,
}

impl Button {
    /// What a press of this button is.
    pub fn action(self) -> Action {
        match self {
            Button::Accept => Action::Accept,
            Button::Options => Action::Menu,
            Button::Back => Action::Back,
        }
    }

    /// The toolkit's mark for this button on a pad (`pad`) or a keyboard.
    /// Options leaves the keyboard for the mouse, as the shell does: a menu
    /// is raised with the right button by anybody holding a pointer.
    pub fn glyph(self, pad: bool) -> &'static str {
        match (self, pad) {
            (Button::Accept, true) => "pad-south",
            (Button::Accept, false) => "key-enter",
            (Button::Options, true) => "pad-north",
            (Button::Options, false) => "mouse-right",
            (Button::Back, true) => "pad-east",
            (Button::Back, false) => "key-escape",
        }
    }
}

/// One thing a button does: the word, and the button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub label: String,
    pub button: Button,
}

impl Hint {
    pub fn new(label: impl Into<String>, button: Button) -> Self {
        Self {
            label: label.into(),
            button,
        }
    }
}

/// What the hands are on — a pad or a keyboard and pointer — so the legend
/// pictures the right buttons, and whether the shell wants button hints at
/// all (Settings > System > Button hints).
///
/// The C API does not say which device an action came from, so this follows
/// what LineXinBar writes down in its settings (`controller-in-hand`, read
/// again as actions arrive, at most a few times a second) and, outside the
/// shell, whether a pad is plugged in; a pointer moving or clicking always
/// means the keyboard and mouse.
#[derive(Debug, Clone)]
pub struct Hands {
    pad: bool,
    shown: bool,
    read: Option<Instant>,
}

impl Default for Hands {
    fn default() -> Self {
        Self::new()
    }
}

impl Hands {
    pub fn new() -> Self {
        let settings = shell_settings();
        Self {
            pad: settings
                .as_deref()
                .and_then(|raw| top_level_bool(raw, "controller-in-hand"))
                .unwrap_or_else(pad_plugged_in),
            shown: settings
                .as_deref()
                .and_then(|raw| top_level_bool(raw, "button-hints"))
                .unwrap_or(true),
            read: Some(Instant::now()),
        }
    }

    /// Whether a pad is in hand.
    pub fn pad(&self) -> bool {
        self.pad
    }

    /// Whether the legend is drawn at all.
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// Follow a frame: `actions` arrived, and the pointer may have moved.
    pub fn follow(&mut self, page: &Page, actions: &[Action]) {
        if page.pointer_moved() {
            self.pad = false;
            return;
        }
        if actions.is_empty() {
            return;
        }
        let due = self
            .read
            .is_none_or(|read| read.elapsed() >= Duration::from_millis(250));
        if !due {
            return;
        }
        self.read = Some(Instant::now());
        if let Some(raw) = shell_settings() {
            if let Some(pad) = top_level_bool(&raw, "controller-in-hand") {
                self.pad = pad;
            }
            if let Some(shown) = top_level_bool(&raw, "button-hints") {
                self.shown = shown;
            }
        }
    }

    /// A click (on anything) says the pointer.
    pub fn clicked(&mut self) {
        self.pad = false;
    }
}

/// A row of hints, kept across frames for its pointer targets.
#[derive(Debug, Clone, Default)]
pub struct Legend {
    drawn: Vec<Button>,
}

impl Legend {
    pub fn new() -> Self {
        Self::default()
    }

    /// How tall a row is: what the foot of a page keeps for it.
    pub fn height(page: &Page) -> f32 {
        page.scaled(GLYPH).max(page.line(Text::Caption))
    }

    /// Which button a click on last frame's row landed on.
    pub fn pressed(&self, page: &Page) -> Option<Button> {
        self.drawn
            .iter()
            .enumerate()
            .find(|(index, _)| page.pressed(LEGEND_SPOTS + *index as u32))
            .map(|(_, button)| *button)
    }

    /// Lay `hints` out from `right` leftwards, centred on `middle`; answers
    /// where the row's left end came out (where anything else on that line
    /// has to stop).
    pub fn draw(&mut self, page: &Page, right: f32, middle: f32, hints: &[Hint], pad: bool) -> f32 {
        let glyph = page.scaled(GLYPH);
        let gap = page.scaled(GAP);
        let step = page.scaled(STEP);
        let line = page.line(Text::Caption);
        self.drawn = hints.iter().map(|hint| hint.button).collect();
        let mut at = right;
        for (index, hint) in hints.iter().enumerate().rev() {
            let word = page.measure(Text::Caption, &hint.label);
            let ends = at - glyph - gap;
            // The word and the picture beside it are one target, grown by
            // half a gap all round (well inside the step between pairs).
            page.draw_spot(
                LEGEND_SPOTS + index as u32,
                [
                    ends - word - gap * 0.5,
                    middle - glyph * 0.5 - gap * 0.5,
                    word + gap * 2.0 + glyph,
                    glyph + gap,
                ],
            );
            page.draw_icon(
                [at - glyph, middle - glyph * 0.5, glyph, glyph],
                hint.button.glyph(pad),
            );
            page.draw_label(
                [ends - word, middle - line * 0.5, word, line],
                Text::Caption,
                &hint.label,
                Role::TextSoft,
                Align::Right,
            );
            at = ends - word - step;
        }
        at
    }

    /// Draw nothing (and take no clicks) this frame.
    pub fn clear(&mut self) {
        self.drawn.clear();
    }
}

/// LineXinBar's settings file, `$XDG_CONFIG_HOME/lxb/shell.toml`.
fn shell_settings() -> Option<String> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    std::fs::read_to_string(config.join("lxb").join("shell.toml")).ok()
}

/// A `key = true|false` at the top level of a TOML file (keys after a table
/// header belong to that table).
fn top_level_bool(raw: &str, wanted: &str) -> Option<bool> {
    for line in raw.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.starts_with('[') {
            return None;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().trim_matches('"') == wanted {
            return match value.trim() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
        }
    }
    None
}

/// Whether a game controller is plugged in (a joystick device exists).
fn pad_plugged_in() -> bool {
    std::fs::read_dir("/dev/input").is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().starts_with("js"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_read_at_the_top_level_only() {
        let raw = "accent = \"Indigo\"\ncontroller-in-hand = true # note\n\n[media-sort]\nbutton-hints = false\n";
        assert_eq!(top_level_bool(raw, "controller-in-hand"), Some(true));
        assert_eq!(top_level_bool(raw, "button-hints"), None);
        assert_eq!(
            top_level_bool("button-hints = false", "button-hints"),
            Some(false)
        );
        assert_eq!(top_level_bool("button-hints = maybe", "button-hints"), None);
    }

    #[test]
    fn every_button_has_both_pictures() {
        for button in [Button::Accept, Button::Options, Button::Back] {
            assert!(button.glyph(true).starts_with("pad-"));
            assert!(!button.glyph(false).is_empty());
        }
        assert_eq!(Button::Options.action(), Action::Menu);
    }
}
