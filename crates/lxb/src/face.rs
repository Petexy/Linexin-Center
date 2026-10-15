//! Faces shared by the flow and the column: drawn with the same calls the
//! library's own flow controls are made of, at the same sizes.

use crate::emblem::{self, Emblem};
use crate::page::Page;
use crate::sys::load;
use crate::types::{Align, Metric, Role, Surface, Text};

/// `menu::ROW` and `menu::ROW_PADDING`, where the toolkit does not answer.
const MENU_ROW: f32 = 64.0;
const ROW_PADDING: f32 = 5.0;

/// How tall the flow's button is: a menu row less its padding, scaled.
pub(crate) fn button_height(page: &Page) -> f32 {
    let (row, padding) = match load() {
        Ok(lxb) => (
            // SAFETY: both take nothing and answer plain values.
            lxb.sym
                .lxb_context_menu
                .map_or(MENU_ROW, |function| unsafe { function() }.row),
            lxb.sym
                .lxb_menu_row_padding
                .map_or(ROW_PADDING, |function| unsafe { function() }),
        ),
        Err(_) => (MENU_ROW, ROW_PADDING),
    };
    page.scaled(row - 2.0 * padding)
}

/// A list entry: its mark (or picture), its label, and what it is set to at
/// the far end. No chip — the light behind it is the only thing that says it
/// is chosen, and its words brighten with it, as the flow's `item` does.
pub(crate) fn entry(
    page: &Page,
    rect: [f32; 4],
    emblem: Emblem,
    label: &str,
    value: Option<&str>,
    lit: bool,
) {
    let [x, y, width, height] = rect;
    let left = emblem_before(page, rect, emblem);
    let right = x + width - page.metric(Metric::RowPadding);
    words(page, [left, y, right - left, height], label, value, lit);
}

/// How tall an entry in two lines is: both lines and the row's air.
pub(crate) fn detailed_height(page: &Page) -> f32 {
    page.line(Text::Body) + page.line(Text::Caption) + page.metric(Metric::RowPadding)
}

/// An entry in two lines: the label, and the note under it, quieter; the
/// emblem a little larger than a one-line entry's, as tall as the two.
pub(crate) fn detailed(
    page: &Page,
    rect: [f32; 4],
    emblem: Emblem,
    label: &str,
    note: &str,
    value: Option<&str>,
    lit: bool,
) {
    let pad = page.metric(Metric::RowPadding);
    let gap = page.metric(Metric::Gap);
    let [x, y, width, height] = rect;
    let mut left = x + pad;
    if !emblem.is_empty() {
        let size = (height * 0.62).round();
        emblem::draw(
            page,
            [left, y + (height - size) * 0.5, size, size],
            emblem,
            1.0,
            1.0,
        );
        left += size + gap;
    }
    let right = x + width - pad;
    let mut room = (right - left).max(0.0);
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        let wide = page.measure(Text::Caption, value).min(room * 0.4);
        let shown = page.fit(Text::Caption, value, wide);
        page.draw_label(
            [right - wide, y, wide, height],
            Text::Caption,
            &shown,
            Role::TextSoft,
            Align::Right,
        );
        room = (room - wide - gap).max(0.0);
    }
    let (first, second) = (page.line(Text::Body), page.line(Text::Caption));
    let top = y + (height - first - second) * 0.5;
    let ink = if lit { Role::Text } else { Role::TextSoft };
    let shown = page.fit(Text::Body, label, room);
    page.draw_label(
        [left, top, room, first],
        Text::Body,
        &shown,
        ink,
        Align::Left,
    );
    let shown = page.fit(Text::Caption, note, room);
    page.draw_label(
        [left, top + first, room, second],
        Text::Caption,
        &shown,
        Role::TextSoft,
        Align::Left,
    );
}

/// An entry with a switch at its far end in place of a value.
pub(crate) fn switch_entry(
    page: &Page,
    rect: [f32; 4],
    emblem: Emblem,
    label: &str,
    on: bool,
    lit: bool,
) {
    let pad = page.metric(Metric::RowPadding);
    let gap = page.metric(Metric::Gap);
    let [x, y, width, height] = rect;
    let tall = (height * 0.42).round();
    let wide = page.switch_width(tall);
    let right = x + width - pad;
    page.draw_switch(
        [right - wide, y + (height - tall) * 0.5, wide, tall],
        on,
        1.0,
    );
    let left = emblem_before(page, rect, emblem);
    words(
        page,
        [left, y, (right - wide - gap - left).max(0.0), height],
        label,
        None,
        lit,
    );
}

/// Draw an entry's emblem at its left end; where its words begin.
fn emblem_before(page: &Page, rect: [f32; 4], emblem: Emblem) -> f32 {
    let pad = page.metric(Metric::RowPadding);
    let [x, y, _, height] = rect;
    let left = x + pad;
    if emblem.is_empty() {
        return left;
    }
    let size = (height * 0.5).round();
    emblem::draw(
        page,
        [left, y + (height - size) * 0.5, size, size],
        emblem,
        1.0,
        1.0,
    );
    left + size + page.metric(Metric::Gap)
}

/// An entry's label and value in `rect`, the value at its far end.
fn words(page: &Page, rect: [f32; 4], label: &str, value: Option<&str>, lit: bool) {
    let gap = page.metric(Metric::Gap);
    let [left, y, room, height] = rect;
    let right = left + room;
    let ink = if lit { Role::Text } else { Role::TextSoft };
    let mut room = (right - left).max(0.0);
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        let wide = page.measure(Text::Caption, value).min(room * 0.45);
        let shown = page.fit(Text::Caption, value, wide);
        page.draw_label(
            [right - wide, y, wide, height],
            Text::Caption,
            &shown,
            Role::TextSoft,
            Align::Right,
        );
        room = (room - wide - gap).max(0.0);
    }
    let shown = page.fit(Text::Body, label, room);
    page.draw_label(
        [left, y, room, height],
        Text::Body,
        &shown,
        ink,
        Align::Left,
    );
}

/// How tall a card is: its name, the line under it, and their air.
pub(crate) fn card_height(page: &Page) -> f32 {
    (page.line(Text::Body) + page.line(Text::Caption) + page.metric(Metric::RowPadding) * 1.6)
        .max(page.metric(Metric::RowHeight) * 1.15)
}

/// A card: glass of its own while the light is elsewhere (the light is its
/// surface while it stands on it), its emblem, its name and the line under
/// it, and what it is set to at its far end.
#[allow(clippy::too_many_arguments)]
pub(crate) fn card(
    page: &Page,
    rect: [f32; 4],
    emblem: Emblem,
    title: &str,
    note: &str,
    value: Option<&str>,
    lit: bool,
    accent: bool,
) {
    if !lit {
        let (role, alpha) = if accent {
            (Role::AccentSoft, 0.45)
        } else {
            (Role::Glass, 0.5)
        };
        page.draw_card(rect, Surface::Control, role, alpha);
    }
    let pad = page.metric(Metric::RowPadding);
    let gap = page.metric(Metric::Gap);
    let [x, y, width, height] = rect;
    let mut left = x + pad;
    if !emblem.is_empty() {
        let size = (height * 0.52).round().min(page.metric(Metric::ItemIcon));
        emblem::draw(
            page,
            [left, y + (height - size) * 0.5, size, size],
            emblem,
            1.0,
            1.0,
        );
        left += size + gap;
    }
    let right = x + width - pad;
    let mut room = (right - left).max(0.0);
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        let wide = page.measure(Text::Caption, value).min(room * 0.4);
        let shown = page.fit(Text::Caption, value, wide);
        // On an accent card the accent would not be read: there it is ink.
        let ink = if lit || accent {
            Role::Text
        } else {
            Role::AccentSoft
        };
        page.draw_label(
            [right - wide, y, wide, height],
            Text::Caption,
            &shown,
            ink,
            Align::Right,
        );
        room = (room - wide - gap).max(0.0);
    }
    let (first, second) = (page.line(Text::Body), page.line(Text::Caption));
    let top = y + (height - first - second) * 0.5;
    let shown = page.fit(Text::Body, title, room);
    page.draw_label(
        [left, top, room, first],
        Text::Body,
        &shown,
        Role::Text,
        Align::Left,
    );
    if !note.is_empty() {
        let shown = page.fit(Text::Caption, note, room);
        page.draw_label(
            [left, top + first, room, second],
            Text::Caption,
            &shown,
            Role::TextSoft,
            Align::Left,
        );
    }
}

/// A gallery's thumbnail: the picture contained in its cell, the one shown
/// large on glass of its own and the others quieter.
pub(crate) fn thumbnail(
    page: &Page,
    rect: [f32; 4],
    path: &std::path::Path,
    current: bool,
    lit: bool,
) {
    if current && !lit {
        page.draw_card(rect, Surface::Control, Role::Glass, 0.6);
    }
    let inset = page.metric(Metric::RowPadding) * 0.35;
    let inner = [
        rect[0] + inset,
        rect[1] + inset,
        (rect[2] - inset * 2.0).max(0.0),
        (rect[3] - inset * 2.0).max(0.0),
    ];
    let radius = page.metric(Metric::TileRadius).min(inner[3] * 0.25);
    let alpha = if current || lit { 1.0 } else { 0.5 };
    if !page.draw_picture(inner, radius, path, crate::types::Fit::Contain, alpha) {
        page.redraw_within(0.05);
    }
}

/// How much of a second the caret shows of each.
const CARET_ON: f32 = 0.55;

/// A field: control glass while the light is elsewhere (the light is its
/// surface while it stands in it), an emblem, what is typed or the hint,
/// and a blinking caret while it takes text. What does not fit shows its
/// end, which is where the typing is.
#[allow(clippy::too_many_arguments)]
pub(crate) fn field(
    page: &Page,
    rect: [f32; 4],
    emblem: Emblem,
    hint: &str,
    text: &str,
    secret: bool,
    typing: bool,
    seconds: f32,
) {
    if !typing {
        page.draw_card(rect, Surface::Control, Role::Glass, 0.55);
    }
    let [x, y, width, height] = rect;
    let left = emblem_before(page, rect, emblem);
    let right = x + width - page.metric(Metric::RowPadding);
    let room = (right - left).max(0.0);
    if text.is_empty() {
        let hint = page.fit(Text::Body, hint, room);
        page.draw_label(
            [left, y, room, height],
            Text::Body,
            &hint,
            Role::TextSoft,
            Align::Left,
        );
    }
    let written: String = if secret {
        "\u{2022}".repeat(text.chars().count())
    } else {
        text.to_owned()
    };
    let shown = tail(page, &written, room - page.scaled(6.0));
    if !shown.is_empty() {
        page.draw_label(
            [left, y, room, height],
            Text::Body,
            &shown,
            Role::Text,
            Align::Left,
        );
    }
    if typing && seconds.fract() < CARET_ON {
        let thick = page.scaled(2.0).max(1.0);
        let tall = page.line(Text::Body) * 0.72;
        let after = if shown.is_empty() {
            0.0
        } else {
            page.measure(Text::Body, &shown)
        };
        page.draw_rule(
            [
                left + after + page.scaled(3.0),
                y + (height - tall) * 0.5,
                thick,
                tall,
            ],
            Role::Text,
        );
    }
}

/// The end of `text` that fits in `room`, an ellipsis before it where some
/// was left out.
fn tail(page: &Page, text: &str, room: f32) -> String {
    if text.is_empty() || page.measure(Text::Body, text) <= room {
        return text.to_owned();
    }
    let letters: Vec<char> = text.chars().collect();
    for start in 1..letters.len() {
        let candidate: String = std::iter::once('\u{2026}')
            .chain(letters[start..].iter().copied())
            .collect();
        if page.measure(Text::Body, &candidate) <= room {
            return candidate;
        }
    }
    String::new()
}

/// The flow's `item`: its label alone, brighter while the light is on it.
pub(crate) fn item(page: &Page, rect: [f32; 4], label: &str, lit: bool) {
    let pad = page.metric(Metric::RowPadding);
    let ink = if lit { Role::Text } else { Role::TextSoft };
    let label = fitted(page, rect, Text::Body, label);
    page.draw_label(
        [rect[0] + pad, rect[1], rect[2] - 2.0 * pad, rect[3]],
        Text::Body,
        &label,
        ink,
        Align::Left,
    );
}

/// `label` cut to what a control as wide as `rect` holds inside its padding.
pub(crate) fn fitted(page: &Page, rect: [f32; 4], text: Text, label: &str) -> String {
    let room = rect[2] - 2.0 * page.metric(Metric::RowPadding);
    page.fit(text, label, room.max(0.0))
}

/// Whether a band `height` tall starting at `top` lies entirely in `clip`.
pub(crate) fn inside(top: f32, height: f32, clip: [f32; 2]) -> bool {
    top >= clip[0] - 0.5 && top + height <= clip[1] + 0.5
}

/// `rect` moved up or down as little as it takes to stand inside `clip`.
pub(crate) fn held_in(rect: [f32; 4], clip: [f32; 2]) -> [f32; 4] {
    let lowest = (clip[1] - rect[3]).max(clip[0]);
    [rect[0], rect[1].clamp(clip[0], lowest), rect[2], rect[3]]
}

/// Seconds since the last frame, from the page's own clock.
pub(crate) fn delta(last: &mut Option<f32>, now: f32) -> f32 {
    let dt = last.map_or(0.0, |last| (now - last).clamp(0.0, 0.1));
    *last = Some(now);
    dt
}
