//! The column of pages down the side of a multi-page application, as
//! DistriBumpy draws its shelves: a sidebar card of glass standing on the
//! window's pane, an entry per page at the height the language gives a row
//! (its mark at the size a list uses, its name, a count at the far end), the
//! light gliding between them, and the chosen entry still marked while the
//! light is in the page beside it.

use crate::emblem::{self, Emblem};
use crate::face;
use crate::layout::glide;
use crate::list::{Laid, Span};
use crate::page::Page;
use crate::types::{Action, Align, Metric, Role, Sound, Surface, Text};
use std::path::Path;

/// Where a [`Column`] numbers its entries' pointer spots from.
pub const COLUMN_SPOTS: u32 = 0x4C59_0000;

/// How strongly the column still marks its chosen entry while the light is
/// in the page beside it.
pub const QUIET: f32 = 0.38;

/// How much of the row beyond each end of a list that scrolls is left
/// showing, as a share of a row (DistriBumpy's `PEEK`).
const PEEK: f32 = 0.45;

/// How much larger the chosen entry's mark is: at a glance it is the size
/// that says which one it is.
const CHOSEN_MARK: f32 = 1.06;

/// How large an entry's mark or picture is, as a share of a list's mark
/// (`ItemIcon`). The light's capsule has rounded ends as round as the row is
/// tall: at the list's size a square icon's corners reach that curve, so the
/// column's stand inside it, clear of the glass's edge.
const EMBLEM: f32 = 0.78;

/// One entry of the column.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ColumnEntry<'a> {
    /// One of the shell's marks (`lxb_glyph_name`), or empty.
    pub mark: &'a str,
    /// The entry's own picture (an application's icon), drawn instead of the
    /// mark where the toolkit draws pictures. See [`Emblem`].
    pub picture: Option<&'a Path>,
    pub label: &'a str,
    /// Written small at the far end (a count), cut to its room.
    pub value: Option<&'a str>,
    /// A rule stands above it: it begins a group of its own, as
    /// DistriBumpy's shelves after Installed do.
    pub parted: bool,
}

/// Everything the column shows, in the words of the moment.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ColumnContent<'a> {
    pub entries: &'a [ColumnEntry<'a>],
    /// An entry standing at the bottom of the card under a rule, out of the
    /// list (it does not scroll away with it): the GTK sidebar's bottom bar.
    /// Its index follows the entries'.
    pub foot: Option<ColumnEntry<'a>>,
}

/// What [`Column::input`] made of a frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColumnInput {
    /// The chosen entry changed (show its page).
    pub moved: bool,
    /// The light should go into the chosen entry's page: Accept, Submit or
    /// Right, or a click on the chosen entry.
    pub enter: bool,
    /// Back was pressed with the light in the column: ask whether to leave.
    pub back: bool,
    /// The column was clicked while the page had the light: the light comes
    /// back to the column (onto the clicked entry).
    pub reclaimed: bool,
    /// Menu, Previous and Next, which the column has no meaning for.
    pub unhandled: Vec<Action>,
    /// Actions that came after the light left for the page, in order: the
    /// page's.
    pub for_page: Vec<Action>,
}

/// The column, kept across frames.
///
/// Up and Down walk it while it has the light (the Move sound, silence at
/// the ends). A pointer moving onto an entry chooses it, silently, and the
/// list does not scroll for it (a list moving under a pointer would choose
/// again); a click on the chosen entry enters its page, a click on another
/// carries the light there first. While the page has the light the chosen
/// entry keeps a quiet capsule, and a click on any entry takes the light
/// back. The light is the page's own (`lxb_page_glide`), shared with
/// [`Flow`](crate::Flow), so it travels between the column and the page.
#[derive(Debug, Clone)]
pub struct Column {
    selected: usize,
    laid: Laid,
    scroll: f32,
    speed: f32,
    target: f32,
    placed: bool,
    /// Whether the view follows the chosen entry (not after a hover).
    follow: bool,
    spots: Vec<usize>,
    base: u32,
    seconds: Option<f32>,
    /// Whether the pointer moving onto an entry while the page has the light
    /// brings the light back (see [`Column::set_hover_reclaims`]).
    hover_reclaims: bool,
}

impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}

impl Column {
    pub fn new() -> Self {
        Self {
            selected: 0,
            laid: Laid::default(),
            scroll: 0.0,
            speed: 0.0,
            target: 0.0,
            placed: false,
            follow: true,
            spots: Vec::new(),
            base: COLUMN_SPOTS,
            seconds: None,
            hover_reclaims: false,
        }
    }

    /// Whether the pointer moving onto an entry while the page has the light
    /// takes the light back onto it, as a click does: the light following
    /// the pointer across the screen. Off on a window where the column and
    /// the page slide (the view would move under the pointer).
    pub fn set_hover_reclaims(&mut self, reclaims: bool) {
        self.hover_reclaims = reclaims;
    }

    /// The chosen entry.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Choose an entry (the view follows it).
    pub fn select(&mut self, index: usize) {
        self.selected = index;
        self.follow = true;
    }

    /// Act on a frame: the pointer always, and `actions` while the column
    /// has the light (`active`). `count` is how many entries there are.
    pub fn input(
        &mut self,
        page: &Page,
        actions: &[Action],
        count: usize,
        active: bool,
    ) -> ColumnInput {
        let mut input = ColumnInput::default();
        if let Some(index) = self
            .spots
            .iter()
            .copied()
            .find(|index| page.pressed(self.base + *index as u32))
            .filter(|index| *index < count)
        {
            // The library sounded the press already.
            if !active {
                input.reclaimed = true;
            } else if index == self.selected {
                input.enter = true;
            }
            input.moved = index != self.selected;
            self.selected = index;
            self.follow = true;
        } else if let Some(index) = page
            .hovered()
            .and_then(|id| id.checked_sub(self.base))
            .map(|index| index as usize)
            .filter(|index| *index < count)
        {
            // Hovering is selecting, silently, and the list stays where it is.
            if active && index != self.selected {
                self.selected = index;
                self.follow = false;
                input.moved = true;
            } else if !active && self.hover_reclaims {
                input.reclaimed = true;
                input.moved = index != self.selected;
                self.selected = index;
                self.follow = false;
            }
        }

        let mut light_here = active && !input.enter;
        for &action in actions {
            if !light_here {
                input.for_page.push(action);
                continue;
            }
            match action {
                Action::Up if self.selected > 0 => {
                    self.selected -= 1;
                    self.follow = true;
                    input.moved = true;
                    page.play(Sound::Move);
                }
                Action::Down if self.selected + 1 < count => {
                    self.selected += 1;
                    self.follow = true;
                    input.moved = true;
                    page.play(Sound::Move);
                }
                Action::Up | Action::Down | Action::Left => {}
                Action::Accept | Action::Submit if count > 0 => {
                    input.enter = true;
                    light_here = false;
                    page.play(Sound::Press);
                }
                Action::Right if count > 0 => {
                    input.enter = true;
                    light_here = false;
                    page.play(Sound::Move);
                }
                Action::Back => {
                    input.back = true;
                    light_here = false;
                }
                Action::Accept | Action::Submit | Action::Right => {}
                other => input.unhandled.push(other),
            }
        }
        if count > 0 {
            self.selected = self.selected.min(count - 1);
        }
        input
    }

    /// Draw the column as a sidebar card filling `rect`. `active` is whether
    /// it has the light.
    pub fn draw(&mut self, page: &Page, rect: [f32; 4], content: &ColumnContent, active: bool) {
        let dt = face::delta(&mut self.seconds, page.seconds());
        // A surface of its own rather than rows on the page: a place to
        // stand, not something on offer.
        page.draw_card(rect, Surface::Sidebar, Role::Glass, 0.40);
        let inset = page.metric(Metric::PanelPadding) * 0.5;
        let inside = [
            rect[0] + inset,
            rect[1] + inset,
            (rect[2] - 2.0 * inset).max(0.0),
            (rect[3] - 2.0 * inset).max(0.0),
        ];
        let count = content.entries.len();
        let total = count + usize::from(content.foot.is_some());
        if total > 0 {
            self.selected = self.selected.min(total - 1);
        }

        let row = page.metric(Metric::RowHeight).min(inside[3].max(1.0));
        let gap = page.metric(Metric::Gap);
        let pad = page.metric(Metric::RowPadding) * 0.7;
        let mark = (page.metric(Metric::ItemIcon) * EMBLEM).min(row);
        // The foot stands at the bottom of the card under a rule; the list
        // has the rest.
        // The foot keeps half a peek of air under it, as the list's last row
        // has over the card's edge.
        let foot_at = content.foot.map(|_| {
            let under = (row * PEEK * 0.5).min((inside[3] - row).max(0.0));
            [
                inside[0],
                inside[1] + inside[3] - row - under,
                inside[2],
                row,
            ]
        });
        let room_for_list = match foot_at {
            Some(foot) => [
                inside[0],
                inside[1],
                inside[2],
                (foot[1] - gap - inside[1]).max(row.min(inside[3])),
            ],
            None => inside,
        };

        // The list is cut to a peek, a whole number of rows, and a peek;
        // what is left over is air at the two ends.
        let peek = row * PEEK;
        let fits = (((room_for_list[3] - peek * 2.0) / row).floor() as usize).max(1);
        let room = (peek * 2.0 + row * fits as f32).min(room_for_list[3]);
        let air = (room_for_list[3] - room) * 0.5;
        let list = [
            room_for_list[0],
            room_for_list[1] + air,
            room_for_list[2],
            room,
        ];
        let top = list[1] + peek;
        self.laid = Laid {
            spans: (0..count)
                .map(|index| Span {
                    x: 0.0,
                    y: index as f32 * row,
                    w: inside[2],
                    h: row,
                })
                .collect(),
            content: count as f32 * row,
            view: row * fits as f32,
        };
        if self.follow && self.selected < count {
            self.target = self.laid.follow(self.selected, self.target, 0.0);
        }
        self.target = self.target.clamp(0.0, self.laid.furthest());
        let target = self.target;
        let jumped = !self.placed || page.low_end();
        if jumped {
            self.scroll = target;
            self.speed = 0.0;
            self.placed = true;
        } else {
            glide(&mut self.scroll, &mut self.speed, target, dt);
        }

        // Rows may stand in the peeks and the air, never past the glass.
        let clip = [room_for_list[1], room_for_list[1] + room_for_list[3]];
        self.spots.clear();
        let chosen = match (self.laid.spans.get(self.selected), foot_at) {
            (Some(span), _) => Some(face::held_in(
                span.on_screen([inside[0], top], self.scroll),
                clip,
            )),
            (None, Some(foot)) if self.selected == count => Some(foot),
            _ => None,
        };
        if let Some(chosen) = chosen {
            if active {
                let strength = page.press(true).light_strength();
                let at = if jumped {
                    page.place(chosen, strength)
                } else {
                    page.glide(chosen, strength)
                };
                page.light_at(at);
            } else {
                page.draw_selection(chosen, QUIET);
            }
        }
        for (index, entry) in content.entries.iter().enumerate() {
            let at = self.laid.spans[index].on_screen([inside[0], top], self.scroll);
            if !face::inside(at[1], at[3], clip) {
                continue;
            }
            if entry.parted && index > 0 {
                // A rule needs something above it; scrolled to where that is
                // gone it would part nothing.
                let thick = page.scaled(1.0).max(1.0);
                if at[1] - thick >= clip[0] {
                    page.draw_rule([at[0] + pad, at[1], at[2] - pad * 2.0, thick], Role::Rim);
                }
            }
            self.entry(page, at, entry, index, mark);
        }
        if let (Some(foot), Some(at)) = (&content.foot, foot_at) {
            let thick = page.scaled(1.0).max(1.0);
            let line = (at[1] - gap * 0.5).round();
            page.draw_rule([at[0] + pad, line, at[2] - pad * 2.0, thick], Role::Rim);
            self.entry(page, at, foot, count, mark);
        }

        let furthest = self.laid.furthest();
        if furthest > 0.5 {
            let band = peek + row * 0.5;
            let up = (self.scroll / band).clamp(0.0, 1.0);
            let down = ((furthest - self.scroll) / band).clamp(0.0, 1.0);
            page.draw_soft_edges(list, band, up, down);
        }
        if (self.scroll - target).abs() > 0.5 || self.speed.abs() > 0.5 {
            page.redraw_within(0.0);
        }
    }

    /// One entry standing at `at`: its spot, its emblem, its name and count.
    fn entry(&mut self, page: &Page, at: [f32; 4], entry: &ColumnEntry, index: usize, mark: f32) {
        page.draw_spot(self.base + index as u32, at);
        self.spots.push(index);
        let pad = page.metric(Metric::RowPadding) * 0.7;
        let chosen = index == self.selected;
        let value = entry.value.filter(|value| !value.is_empty());
        let value_room = value.map_or(0.0, |value| page.measure(Text::Caption, value) + pad * 1.6);
        let mut left = pad * 0.8;
        let emblem = Emblem::with_picture(entry.picture, entry.mark);
        if !emblem.is_empty() {
            let size = if chosen { mark * CHOSEN_MARK } else { mark };
            emblem::draw(
                page,
                [
                    at[0] + left + (mark - size) * 0.5,
                    at[1] + (at[3] - size) * 0.5,
                    size,
                    size,
                ],
                emblem,
                1.0,
                1.0,
            );
            left += mark + pad * 0.8;
        }
        let words = (at[2] - left - pad - value_room).max(pad);
        let label = page.fit(Text::Body, entry.label, words);
        page.draw_label(
            [at[0] + left, at[1], words, at[3]],
            Text::Body,
            &label,
            if chosen { Role::Text } else { Role::TextSoft },
            Align::Left,
        );
        if let Some(value) = value {
            page.draw_label(
                [at[0], at[1], at[2] - pad, at[3]],
                Text::Caption,
                value,
                if chosen { Role::Text } else { Role::AccentSoft },
                Align::Right,
            );
        }
    }

    /// How wide the column's card must be for every entry's name and count
    /// to stand on one line beside its mark (window pixels), never narrower
    /// than DistriBumpy's 348-point panel.
    pub fn fitting_width(page: &Page, content: &ColumnContent) -> f32 {
        let pad = page.metric(Metric::RowPadding) * 0.7;
        let mark = page.metric(Metric::ItemIcon) * EMBLEM;
        let names = content
            .entries
            .iter()
            .chain(content.foot.as_ref())
            .map(|entry| {
                let value = entry
                    .value
                    .filter(|value| !value.is_empty())
                    .map_or(0.0, |value| page.measure(Text::Caption, value) + pad * 1.6);
                page.measure(Text::Body, entry.label) + value
            })
            .fold(0.0f32, f32::max);
        let wanted = names + mark + pad * 2.6 + page.metric(Metric::PanelPadding) + 1.0;
        page.scaled(348.0).max(wanted)
    }
}

/// The sound for the light coming back from a page that returned
/// `LXB_PAGE_LEAVE` on these actions: Back's own where Back sent it, a move
/// otherwise (Left, or the page's own reason).
pub fn hand_back_sound(actions: &[Action]) -> Sound {
    if actions.contains(&Action::Back) {
        Sound::Back
    } else {
        Sound::Move
    }
}
