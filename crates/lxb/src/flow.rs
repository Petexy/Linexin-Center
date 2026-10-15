//! A driven page made of flow controls, that scrolls to keep its light in
//! view.

use crate::emblem::{self, Emblem};
use crate::face;
use crate::layout::glide;
use crate::list::{Laid, Span, Step, Walk};
use crate::page::Page;
use crate::types::{Action, Align, Metric, Press, Role, Sound, Text};
use crate::wrap::wrap;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

/// Where a [`Flow`] numbers the pointer spots of its controls from: the
/// control at index `n` is spot `FLOW_SPOTS + n`. Clear of the library's own
/// flow numbering (from 0), of the column's ([`COLUMN_SPOTS`](crate::COLUMN_SPOTS))
/// and of the tours' `0x200`.
pub const FLOW_SPOTS: u32 = 0x4C58_0000;

/// The side of the emblem before a page's name (nothing without one).
fn head_emblem(page: &Page, emblem: bool) -> f32 {
    if emblem {
        (page.metric(Metric::ItemIcon) * HEAD_EMBLEM).round()
    } else {
        0.0
    }
}

/// How tall a page's head is with `lines` of its name beside an emblem
/// `size` tall, without the air under it.
fn head_band(page: &Page, lines: usize, size: f32) -> f32 {
    (page.line(Text::Title) * lines.max(1) as f32).max(size)
}

/// How tall a gallery's thumbnails are beside rows `row` tall.
pub(crate) fn gallery_strip(row: f32) -> f32 {
    (row * 1.25).round()
}

/// How much larger than a list's mark the emblem before a page's name is.
const HEAD_EMBLEM: f32 = 1.3;

/// The most lines a page's name takes at its head.
const HEAD_LINES: usize = 2;

/// What a [`FlowFrame::field`] made of a frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldInput {
    /// Something was typed into it or rubbed out.
    pub changed: bool,
    /// Accept was pressed in it (Enter, or the pad's South): what a form's
    /// Enter does. A click only puts the light there.
    pub submitted: bool,
}

/// Why a page gives the light back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leave {
    /// Back: nothing of the page's own was left to close.
    Back,
    /// Left, with nothing to the left of the light.
    Left,
}

/// What [`Flow::input`] made of a frame's actions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Input {
    /// The light should go back to whoever handed it over: return
    /// `LXB_PAGE_LEAVE`. Actions after it were not looked at.
    pub leave: Option<Leave>,
    /// What the flow has no meaning for (Right, Menu, Previous, Next, and
    /// Left when [`Flow::left_leaves`] is off), in order, for the page.
    pub unhandled: Vec<Action>,
}

/// A page of flow controls, kept across frames.
///
/// Up and Down walk the light over the controls the page drew on the last
/// frame (with the Move sound, and silence where it cannot move); Accept or
/// Submit, or a pointer press, makes the control return `true` once; the
/// pointer moving onto a control lights it, silently, without scrolling; the
/// content scrolls inside the page's rectangle to keep the light in view,
/// gliding there, and leaves out what lies outside (softening the ends).
/// Back, and Left, are reported for the page to give the light back.
///
/// ```ignore
/// let input = flow.input(&page, &actions);          // only when focused
/// let mut f = flow.begin(&page, rect, focused);
/// f.head("setting-info", &tr("System"));
/// if f.row_value(&tr("Kernel"), &kernel) { /* pressed */ }
/// f.finish();
/// if input.leave.is_some() { LXB_PAGE_LEAVE } else { LXB_PAGE_STAY }
/// ```
///
/// The controls are drawn with `lxb_draw_row`/`lxb_draw_button`/labels at the
/// flow's own sizes, numbered with spots ([`FLOW_SPOTS`]) and lit with the
/// page's light (`lxb_page_glide`) — the same light the Center's column uses,
/// so it travels from the column into the page and back. Draw no
/// `lxb_page_row`-style controls of the library's own on the same frame.
#[derive(Debug, Clone)]
pub struct Flow {
    walk: Walk,
    laid: Laid,
    laid_once: bool,
    scroll: f32,
    speed: f32,
    placed: bool,
    was_lit: bool,
    /// Whether this frame's actions were taken already (`input`).
    taken: bool,
    pending: Option<usize>,
    spots: Vec<usize>,
    base: u32,
    left_leaves: bool,
    bleed: Option<f32>,
    seconds: Option<f32>,
    lines: HashMap<u64, Arc<[String]>>,
    /// How tall the heading pinned above the content was on the last frame
    /// (see [`FlowFrame::head`]).
    pinned: f32,
    /// Whether the view follows the light (not after a hover: a list moving
    /// under a pointer would light something else).
    follow: bool,
    /// Whether the pending press came from the pointer.
    pointer_pressed: bool,
    /// How the light was shaped on each control last frame: 0 for the
    /// capsule a row wears, else the corner radius of a card or a picture.
    shapes: Vec<f32>,
    /// Whether the pointer reaches the controls while the flow is unlit
    /// (a panel beside the page: see [`SidePanel`](crate::SidePanel)).
    pointable: bool,
    /// The pointer moved onto (or pressed) one of the controls this frame
    /// while the flow was unlit.
    claimed: bool,
}

impl Default for Flow {
    fn default() -> Self {
        Self::new()
    }
}

impl Flow {
    pub fn new() -> Self {
        Self {
            walk: Walk {
                focus: Some(0),
                ..Walk::default()
            },
            laid: Laid::default(),
            laid_once: false,
            scroll: 0.0,
            speed: 0.0,
            placed: false,
            was_lit: false,
            taken: false,
            pending: None,
            spots: Vec::new(),
            base: FLOW_SPOTS,
            left_leaves: true,
            bleed: None,
            seconds: None,
            lines: HashMap::new(),
            pinned: 0.0,
            follow: true,
            pointer_pressed: false,
            shapes: Vec::new(),
            pointable: false,
            claimed: false,
        }
    }

    /// Number this flow's pointer spots from `base` instead of
    /// [`FLOW_SPOTS`] (for a second flow drawn on the same frame).
    pub fn spots_from(mut self, base: u32) -> Self {
        self.base = base;
        self
    }

    /// Whether Left gives the light back (the default); off, it is passed
    /// on in [`Input::unhandled`] for a page that uses it.
    pub fn left_leaves(mut self, leaves: bool) -> Self {
        self.left_leaves = leaves;
        self
    }

    /// How far beyond the page's rectangle content may still be drawn while
    /// it scrolls away (by default the pane's padding, `PanelPadding`).
    pub fn bleed(mut self, bleed: f32) -> Self {
        self.bleed = Some(bleed);
        self
    }

    /// Whether the pointer reaches the controls while the flow is unlit,
    /// and claims the light by moving onto one ([`Flow::claimed`]).
    pub fn set_pointable(&mut self, pointable: bool) {
        self.pointable = pointable;
    }

    /// Whether the pointer moved onto one of the controls (or pressed one)
    /// on the last frame drawn unlit: the light belongs here now.
    pub fn claimed(&self) -> bool {
        self.claimed
    }

    /// The control the light is on; `None` while there is none to be on, or
    /// it stepped past the first or last to read what lies beyond.
    pub fn focused(&self) -> Option<usize> {
        self.walk.focus
    }

    /// Put the light on control `index`; the view follows it.
    pub fn focus(&mut self, index: usize) {
        self.walk.put(index);
        self.follow = true;
    }

    /// How many controls the page drew on the last frame.
    pub fn count(&self) -> usize {
        self.laid.count()
    }

    /// How tall a page's `head` stands, the air under it included, with
    /// `title` on a page `width` wide and an emblem before the name or not:
    /// what a panel beside the page can know before the page is drawn.
    pub fn head_height(page: &Page, title: &str, width: f32, emblem: bool) -> f32 {
        let gap = page.metric(Metric::Gap);
        let size = head_emblem(page, emblem);
        let room = (width - if size > 0.0 { size + gap } else { 0.0 }).max(0.0);
        let lines = wrap(title, room, |candidate| {
            page.measure(Text::Title, candidate)
        })
        .len();
        head_band(page, lines.min(HEAD_LINES), size) + gap * 1.5
    }

    /// How tall the heading pinned at the page's top is (0 without one):
    /// what a panel beside the page leaves above itself to stand level with
    /// the page's content.
    pub fn pinned(&self) -> f32 {
        self.pinned
    }

    /// How far the content is scrolled.
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// Stand the view where it is going on the next frame, without gliding
    /// there (a page shown again after a while, a window resized).
    pub fn snap(&mut self) {
        self.placed = false;
    }

    /// Back to the top, the light on the first control.
    pub fn reset(&mut self) {
        self.walk = Walk {
            focus: Some(0),
            ..Walk::default()
        };
        self.scroll = 0.0;
        self.speed = 0.0;
        self.placed = false;
        self.pending = None;
        self.follow = true;
    }

    /// Act on a frame's actions (the page's, while it has the light). Call it
    /// before [`Flow::begin`].
    pub fn input(&mut self, page: &Page, actions: &[Action]) -> Input {
        let mut input = Input::default();
        self.pending = None;
        if actions.is_empty() {
            return input;
        }
        if !self.laid_once {
            // Nothing laid out yet to walk over (the page's first frame).
            return input_without_layout(actions, self.left_leaves);
        }
        if !self.was_lit && !self.taken {
            self.walk.arrive(&self.laid, self.scroll);
        }
        self.taken = true;
        let step = page.metric(Metric::RowHeight).max(1.0);
        let margin = step * 0.5;
        for &action in actions {
            if input.leave.is_some() {
                break;
            }
            match action {
                Action::Up | Action::Down => {
                    if self.walk.step(action == Action::Down, &self.laid, step) == Step::Moved {
                        page.play(Sound::Move);
                    }
                    self.follow = true;
                    self.walk.settle(&self.laid, margin);
                }
                Action::Accept | Action::Submit => {
                    if let Some(focus) = self.walk.focus.filter(|focus| *focus < self.laid.count())
                    {
                        self.pending = Some(focus);
                        self.pointer_pressed = false;
                        page.play(Sound::Press);
                    }
                }
                Action::Left | Action::Right => {
                    let right = action == Action::Right;
                    if self.walk.side(right, &self.laid) {
                        // A grid's next card, or the one before it.
                        page.play(Sound::Move);
                        self.follow = true;
                        self.walk.settle(&self.laid, margin);
                    } else if !right && self.left_leaves {
                        input.leave = Some(Leave::Left);
                    } else {
                        input.unhandled.push(action);
                    }
                }
                Action::Back => input.leave = Some(Leave::Back),
                other => input.unhandled.push(other),
            }
        }
        input
    }

    /// Start drawing a frame of the page into `rect` (the page's area). `lit`
    /// is whether the page has the light; without it the page is a preview:
    /// no light, no pointer, no input.
    pub fn begin<'f, 'p>(
        &'f mut self,
        page: &'f Page<'p>,
        rect: [f32; 4],
        lit: bool,
    ) -> FlowFrame<'f, 'p> {
        let dt = face::delta(&mut self.seconds, page.seconds());
        let row = page.metric(Metric::RowHeight);
        let margin = row * 0.5;
        // LineXinBar's on-screen keyboard stands over the window's foot while
        // a field is typed into: the page ends above it, so the light (and
        // the field it stands in) is scrolled clear of it.
        let rect = match page.board().filter(|_| lit) {
            Some(board) if board[1] > rect[1] + row => [
                rect[0],
                rect[1],
                rect[2],
                rect[3].min(board[1] - rect[1] - margin),
            ],
            _ => rect,
        };
        let whole = rect;
        // The heading the page pinned last frame keeps its place above the
        // content, which scrolls beneath it.
        let pinned = self.pinned.min(rect[3]);
        let rect = [
            rect[0],
            rect[1] + pinned,
            rect[2],
            (rect[3] - pinned).max(0.0),
        ];
        self.laid.view = rect[3];

        if lit && self.laid_once {
            if !self.was_lit && !self.taken {
                self.walk.arrive(&self.laid, self.scroll);
            }
            if let Some(index) = self
                .spots
                .iter()
                .copied()
                .find(|index| page.pressed(self.base + *index as u32))
            {
                // The library sounded the press already.
                self.walk.put(index);
                self.pending = Some(index);
                self.pointer_pressed = true;
                self.follow = true;
            } else if let Some(index) = page
                .hovered()
                .and_then(|id| id.checked_sub(self.base))
                .map(|index| index as usize)
                .filter(|index| *index < self.laid.count())
            {
                // Hovering is selecting, silently; the view stays where it
                // is, so nothing moves under the pointer.
                self.walk.put(index);
                self.follow = false;
            }
            if self.follow {
                self.walk.settle(&self.laid, margin);
            }
        } else if self.laid_once {
            self.walk.target = self.walk.target.clamp(0.0, self.laid.furthest());
        }
        self.claimed = false;
        if !lit && self.pointable && self.laid_once {
            let pressed = self
                .spots
                .iter()
                .copied()
                .find(|index| page.pressed(self.base + *index as u32));
            let hovered = page
                .hovered()
                .and_then(|id| id.checked_sub(self.base))
                .map(|index| index as usize)
                .filter(|index| *index < self.laid.count());
            if let Some(index) = pressed.or(hovered) {
                self.walk.put(index);
                self.claimed = true;
            }
        }

        let mut jumped = false;
        if !self.placed || page.low_end() {
            if self.laid_once {
                self.scroll = self.walk.target;
                self.speed = 0.0;
                self.placed = true;
                jumped = true;
            }
        } else {
            glide(&mut self.scroll, &mut self.speed, self.walk.target, dt);
        }

        let bleed = self
            .bleed
            .unwrap_or_else(|| page.metric(Metric::PanelPadding));
        // Under a pinned heading nothing scrolls up over it.
        let above = if pinned > 0.0 { 0.0 } else { bleed };
        let mut frame = FlowFrame {
            gap: page.metric(Metric::Gap),
            row,
            pair_names: None,
            clip: [rect[1] - above, rect[1] + rect[3] + bleed],
            bleed,
            flow: self,
            page,
            whole,
            rect,
            pinned: 0.0,
            lit,
            jumped,
            light_drawn: false,
            y: 0.0,
            spans: Vec::new(),
            shapes: Vec::new(),
            spots: Vec::new(),
            grid: None,
            finished: false,
        };
        frame.lay_the_light();
        frame
    }

    fn lines(&mut self, page: &Page, text: &str, room: f32, size: Text) -> Arc<[String]> {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        room.to_bits().hash(&mut hasher);
        size.raw().hash(&mut hasher);
        page.line(size).to_bits().hash(&mut hasher);
        let key = hasher.finish();
        if let Some(lines) = self.lines.get(&key) {
            return lines.clone();
        }
        if self.lines.len() > 512 {
            self.lines.clear();
        }
        // Each line of the text wrapped on its own, so a list or a heading
        // in a reply keeps its lines; runs of empty lines are one empty line.
        let mut lines: Vec<String> = Vec::new();
        for line in text.trim_matches('\n').split('\n') {
            let wrapped = wrap(line, room, |candidate| page.measure(size, candidate));
            if wrapped.is_empty() {
                if lines.last().is_some_and(|last| !last.is_empty()) {
                    lines.push(String::new());
                }
            } else {
                lines.extend(wrapped);
            }
        }
        let lines: Arc<[String]> = lines.into();
        self.lines.insert(key, lines.clone());
        lines
    }
}

/// Back and Left still mean leaving before the page has been laid out.
fn input_without_layout(actions: &[Action], left_leaves: bool) -> Input {
    let mut input = Input::default();
    for &action in actions {
        match action {
            Action::Back => input.leave = Some(Leave::Back),
            Action::Left if left_leaves => input.leave = Some(Leave::Left),
            Action::Up | Action::Down | Action::Accept | Action::Submit => {}
            other => input.unhandled.push(other),
        }
        if input.leave.is_some() {
            break;
        }
    }
    input
}

/// Cells the controls of a grid are laid in, row by row.
#[derive(Debug, Clone, Copy)]
struct Grid {
    columns: usize,
    width: f32,
    height: f32,
    gap: f32,
    /// The next cell's column, and its row's top in the content.
    column: usize,
    top: f32,
}

/// One frame of a [`Flow`], laying itself out down the page from the top of
/// its rectangle. Finishes (softens the ends, remembers the layout) when
/// dropped, or with [`FlowFrame::finish`].
pub struct FlowFrame<'f, 'p> {
    flow: &'f mut Flow,
    page: &'f Page<'p>,
    /// The page's whole area, the pinned heading's included.
    whole: [f32; 4],
    /// Where the content goes: the area under the pinned heading.
    rect: [f32; 4],
    /// How tall the heading pinned this frame is.
    pinned: f32,
    lit: bool,
    jumped: bool,
    light_drawn: bool,
    /// How far down the content the next element goes.
    y: f32,
    spans: Vec<Span>,
    /// The light's shape on each control (see [`Flow`]'s `shapes`).
    shapes: Vec<f32>,
    spots: Vec<usize>,
    /// The grid the controls go into, while one is open.
    grid: Option<Grid>,
    clip: [f32; 2],
    bleed: f32,
    gap: f32,
    row: f32,
    /// Width of a pair's name column; see [`FlowFrame::pairs_named`].
    pair_names: Option<f32>,
    finished: bool,
}

impl<'p> FlowFrame<'_, 'p> {
    /// The page being drawn on, for anything drawn by hand.
    pub fn page(&self) -> &Page<'p> {
        self.page
    }

    /// The page's rectangle.
    pub fn rect(&self) -> [f32; 4] {
        self.rect
    }

    /// The page's name, at the top of it.
    pub fn title(&mut self, text: &str) {
        self.line_of(Text::Display, text, Role::Text);
    }

    /// A name for the group under it.
    pub fn heading(&mut self, text: &str) {
        self.line_of(Text::Title, text, Role::Text);
    }

    /// A sentence, wrapped to the page's width.
    pub fn text(&mut self, text: &str) {
        self.paragraph(text, Role::Text);
    }

    /// The same, quieter.
    pub fn note(&mut self, text: &str) {
        self.paragraph(text, Role::TextSoft);
    }

    /// One of the shell's marks, at the size a list uses.
    pub fn icon(&mut self, name: &str) {
        let size = self.page.metric(Metric::ItemIcon);
        if let Some(top) = self.advance(size) {
            self.page.draw_icon([self.rect[0], top, size, size], name);
        }
        self.y += self.gap;
    }

    /// The page's heading: its emblem and its name, as the first thing on
    /// the page.
    ///
    /// Written as DistriBumpy heads a page of its own: the name in the title
    /// size at the top of the page, pinned there while what follows scrolls
    /// beneath it, with the application's own picture (or the mark standing
    /// in for it) before the name, as its pages about one application and
    /// one repository have. A head that is not the first thing on the page is
    /// a heading in the flow.
    pub fn head<'e>(&mut self, emblem: impl Into<Emblem<'e>>, title: &str) {
        if self.y > 0.0 || !self.spans.is_empty() || self.pinned > 0.0 {
            self.heading(title);
            return;
        }
        let emblem = emblem.into();
        let line = self.page.line(Text::Title);
        let size = head_emblem(self.page, !emblem.is_empty());
        let [x, y, width, _] = self.whole;
        let left = if size > 0.0 { x + size + self.gap } else { x };
        let room = (x + width - left).max(0.0);
        // A name too long for one line takes two (a translated one beside a
        // panel); what a third would hold is cut.
        let mut lines: Vec<String> = self
            .flow
            .lines(self.page, title, room, Text::Title)
            .iter()
            .filter(|line| !line.is_empty())
            .cloned()
            .collect();
        if lines.len() > HEAD_LINES {
            let rest = lines.split_off(HEAD_LINES - 1).join(" ");
            lines.push(self.page.fit(Text::Title, &rest, room));
        }
        let words = line * lines.len().max(1) as f32;
        let band = head_band(self.page, lines.len(), size);
        let tall = (band + self.gap * 1.5).min(self.whole[3]);
        if size > 0.0 {
            emblem::draw(
                self.page,
                [x, y + (band - size) * 0.5, size, size],
                emblem,
                1.0,
                1.0,
            );
        }
        let top = y + (band - words) * 0.5;
        for (index, title) in lines.iter().enumerate() {
            self.page.draw_label(
                [left, top + line * index as f32, room, line],
                Text::Title,
                title,
                Role::Text,
                Align::Left,
            );
        }
        self.pinned = tall;
        if (tall - self.flow.pinned).abs() > 0.01 {
            // The first frame with this heading: what follows moves below it
            // now rather than a frame late.
            self.rect = [x, y + tall, width, (self.whole[3] - tall).max(0.0)];
            self.clip = [self.rect[1], self.rect[1] + self.rect[3] + self.bleed];
        }
    }

    /// The hairline the language groups with.
    pub fn rule(&mut self) {
        let thick = self.page.scaled(1.0).max(1.0);
        self.y += self.gap * 0.5;
        if let Some(top) = self.advance(thick) {
            self.page
                .draw_rule([self.rect[0], top, self.rect[2], thick], Role::AccentSoft);
        }
        self.y += self.gap * 0.5;
    }

    /// Air enough for what follows, `reserve` tall, to stand at the foot of
    /// the view while everything fits (a chat's field under a short
    /// conversation); nothing once the page scrolls.
    pub fn push_to_end(&mut self, reserve: f32) {
        self.end_grid();
        let foot = self.rect[3] - reserve;
        if self.y < foot {
            self.y = foot;
        }
    }

    /// One gap's worth of air.
    pub fn gap(&mut self) {
        self.y += self.gap;
    }

    /// Size the name column of the pairs that follow to the widest of
    /// `names` (never narrower than the tour's column, never wider than
    /// 45% of the page), so translated names are not cut while there is room.
    pub fn pairs_named(&mut self, names: &[&str]) {
        let widest = names
            .iter()
            .map(|name| self.page.measure(Text::Body, name))
            .fold(0.0f32, f32::max);
        self.pair_names = Some(widest + self.gap);
    }

    /// A name and its value on one line, the value in a second column (as
    /// the tour's first page writes them); both cut to their room.
    pub fn pair(&mut self, name: &str, value: &str) {
        let line = self.page.line(Text::Body);
        if let Some(top) = self.advance(line) {
            let [x, _, width, _] = self.rect;
            let column = self
                .page
                .metric(Metric::ColumnSpacing)
                .max(self.pair_names.unwrap_or(0.0))
                .min(width * 0.45);
            let name = self.page.fit(Text::Body, name, column - self.gap * 0.5);
            self.page.draw_label(
                [x, top, column, line],
                Text::Body,
                &name,
                Role::Text,
                Align::Left,
            );
            let room = (width - column).max(0.0);
            let value = self.page.fit(Text::Caption, value, room);
            self.page.draw_label(
                [x + column, top, room, line],
                Text::Caption,
                &value,
                Role::TextSoft,
                Align::Left,
            );
        }
    }

    /// Room `height` tall for something drawn by hand: its rectangle when it
    /// is in view, `None` when it scrolled out of it (draw nothing then).
    pub fn space(&mut self, height: f32) -> Option<[f32; 4]> {
        self.advance(height)
            .map(|top| [self.rect[0], top, self.rect[2], height])
    }

    /// A row as wide as the page; `true` on the frame it is pressed.
    pub fn row(&mut self, label: &str) -> bool {
        self.control(self.row, None, 0.0, |page, rect, press| {
            let label = face::fitted(page, rect, Text::Body, label);
            page.draw_row(rect, &label, None, press)
        })
    }

    /// A row with what it is set to under its name.
    pub fn row_value(&mut self, label: &str, value: &str) -> bool {
        self.control(self.row, None, 0.0, |page, rect, press| {
            let label = face::fitted(page, rect, Text::Body, label);
            let value = face::fitted(page, rect, Text::Caption, value);
            page.draw_row(rect, &label, Some(&value), press)
        })
    }

    /// One entry of a list: its label, and the light behind it when lit.
    pub fn item(&mut self, label: &str) -> bool {
        self.control(self.row, None, 0.0, |page, rect, press| {
            face::item(page, rect, label, press.lit())
        })
    }

    /// An entry with a mark (or a picture, see [`Emblem`]) before its label
    /// and, optionally, a value at its far end.
    pub fn entry<'e>(
        &mut self,
        emblem: impl Into<Emblem<'e>>,
        label: &str,
        value: Option<&str>,
    ) -> bool {
        let emblem = emblem.into();
        self.control(self.row, None, 0.0, |page, rect, press| {
            face::entry(page, rect, emblem, label, value, press.lit())
        })
    }

    /// An entry in two lines: its label, and a quieter `note` under it (a
    /// package's description, a conversation's date), with an optional value
    /// at its far end. As tall as the two lines need.
    pub fn detailed<'e>(
        &mut self,
        emblem: impl Into<Emblem<'e>>,
        label: &str,
        note: &str,
        value: Option<&str>,
    ) -> bool {
        let emblem = emblem.into();
        let tall = face::detailed_height(self.page).max(self.row);
        self.control(tall, None, 0.0, |page, rect, press| {
            face::detailed(page, rect, emblem, label, note, value, press.lit())
        })
    }

    /// An entry with a switch at its far end, drawn `on` or off; `true` on
    /// the frame it is pressed (the page turns it over).
    pub fn switch<'e>(&mut self, emblem: impl Into<Emblem<'e>>, label: &str, on: bool) -> bool {
        let emblem = emblem.into();
        self.control(self.row, None, 0.0, |page, rect, press| {
            face::switch_entry(page, rect, emblem, label, on, press.lit())
        })
    }

    /// A field the light types into while it stands on it: `text` takes
    /// what is typed (LineXinBar's on-screen keyboard rises over it with a
    /// controller in hand), `hint` is written where it is empty, `secret`
    /// shows dots for a password. Where the toolkit cannot take text
    /// ([`Page::takes_typing`]) it is drawn but not typed into.
    pub fn field<'e>(
        &mut self,
        emblem: impl Into<Emblem<'e>>,
        hint: &str,
        text: &mut String,
        secret: bool,
    ) -> FieldInput {
        let emblem = emblem.into();
        let index = self.spans.len();
        let typing = self.lit && self.flow.walk.focus == Some(index);
        let mut input = FieldInput::default();
        if typing {
            input.changed = crate::typed::apply(text, &self.page.typed());
        }
        let by_pointer = self.flow.pointer_pressed;
        let seconds = self.page.seconds();
        let mut at = None;
        let shown: &str = text;
        let pressed = self.control(self.row, None, 0.0, |page, rect, _| {
            face::field(page, rect, emblem, hint, shown, secret, typing, seconds);
            at = Some(rect);
        });
        if typing {
            self.page.taking_text(at);
            // The caret blinks.
            self.page.redraw_within(0.5);
        }
        input.submitted = pressed && !by_pointer;
        input
    }

    /// Start a grid: the cards that follow stand in as many columns as fit
    /// at least `least` wide (at most `most`), `height` tall, row by row,
    /// until [`FlowFrame::end_grid`]; the light walks it across and down. How
    /// many columns there are.
    pub fn grid(&mut self, least: f32, most: usize, height: f32) -> usize {
        let room = self.rect[2];
        let columns =
            (((room + self.gap) / (least + self.gap)).floor() as usize).clamp(1, most.max(1));
        let width = (room - self.gap * (columns - 1) as f32) / columns as f32;
        self.open_grid(columns, width, height);
        columns
    }

    /// A grid of cells `width` by `height` from the page's left edge, as
    /// many across as fit (a strip of thumbnails).
    pub fn grid_of(&mut self, width: f32, height: f32) {
        let columns = (((self.rect[2] + self.gap) / (width + self.gap)).floor() as usize).max(1);
        self.open_grid(columns, width.min(self.rect[2]), height);
    }

    fn open_grid(&mut self, columns: usize, width: f32, height: f32) {
        self.end_grid();
        self.grid = Some(Grid {
            columns,
            width,
            height,
            gap: self.gap,
            column: 0,
            top: self.y,
        });
    }

    /// The grid is over: what follows goes under its last row.
    pub fn end_grid(&mut self) {
        if let Some(grid) = self.grid.take() {
            self.y = if grid.column > 0 {
                grid.top + grid.height + grid.gap
            } else {
                grid.top
            };
        }
    }

    /// How tall a card is: its two lines and their air.
    pub fn card_height(&self) -> f32 {
        face::card_height(self.page)
    }

    /// A card of a grid (or as wide as the page outside one): its emblem,
    /// its name, a quieter line under it, and a value at its far end, on a
    /// glass of its own (`accent` tints it, as the GTK page's highlighted
    /// cards). Lit as itself, not as a row. `true` on the frame it is
    /// pressed.
    pub fn card<'e>(
        &mut self,
        emblem: impl Into<Emblem<'e>>,
        title: &str,
        note: &str,
        value: Option<&str>,
        accent: bool,
    ) -> bool {
        let emblem = emblem.into();
        let height = self.card_height();
        let radius = self.page.metric(Metric::CardRadius);
        self.control_shaped(height, None, self.gap * 0.5, radius, |page, rect, press| {
            face::card(page, rect, emblem, title, note, value, press.lit(), accent)
        })
    }

    /// A picture as wide as the page and `height` tall, contained in it, its
    /// corners rounded as a card's: the large picture of a gallery. Not a
    /// control (its thumbnails are).
    pub fn picture_view(&mut self, path: &std::path::Path, height: f32) {
        self.picture_within(path, height, f32::INFINITY);
    }

    /// [`picture_view`](Self::picture_view) at most `width` wide, at the
    /// page's left (a gallery's picture over its thumbnails, which start
    /// there too).
    fn picture_within(&mut self, path: &std::path::Path, height: f32, width: f32) {
        if !self.page.draws_pictures() {
            return;
        }
        let radius = self.page.metric(Metric::CardRadius);
        if let Some([x, y, room, tall]) = self.space(height)
            && !self.page.draw_picture(
                [x, y, room.min(width), tall],
                radius,
                path,
                crate::types::Fit::Contain,
                1.0,
            )
        {
            self.page.redraw_within(0.05);
        }
        self.y += self.gap;
    }

    /// A gallery, as the GTK installers' carousel: the picture `shown`
    /// large, `height` tall, and under it a strip of every picture's
    /// thumbnail; the light on a thumbnail, or a press on one, shows that
    /// picture large. Nothing where the toolkit draws no pictures.
    pub fn gallery(&mut self, pictures: &[std::path::PathBuf], shown: &mut usize, height: f32) {
        if pictures.is_empty() || !self.page.draws_pictures() {
            return;
        }
        *shown = (*shown).min(pictures.len() - 1);
        // The thumbnails' controls come after the large picture, which is
        // not one: the first of them is the next control.
        let first = self.spans.len();
        if pictures.len() > 1
            && self.lit
            && let Some(focus) = self.flow.walk.focus
            && (first..first + pictures.len()).contains(&focus)
        {
            *shown = focus - first;
        }
        // The pictures are a screen's (16:9), as their thumbnails are.
        self.picture_within(&pictures[*shown], height, height * 16.0 / 9.0);
        if pictures.len() > 1 {
            let tall = self.gallery_strip();
            self.grid_of(tall * 16.0 / 9.0, tall);
            for (index, picture) in pictures.iter().enumerate() {
                if self.thumbnail(picture, index == *shown) {
                    *shown = index;
                }
            }
            self.end_grid();
        }
    }

    /// How tall a gallery's thumbnails are.
    pub fn gallery_strip(&self) -> f32 {
        gallery_strip(self.row)
    }

    /// A thumbnail of a gallery, in a grid ([`FlowFrame::grid_of`]): the
    /// picture shown large is `current`, the others are quieter. Lit as
    /// itself. `true` on the frame it is pressed.
    pub fn thumbnail(&mut self, path: &std::path::Path, current: bool) -> bool {
        if !self.page.draws_pictures() {
            return false;
        }
        let radius = self.page.metric(Metric::CardRadius);
        let height = self.grid.map_or(self.row * 1.4, |grid| grid.height);
        self.control_shaped(
            height,
            Some(height * 16.0 / 9.0),
            self.gap,
            radius,
            |page, rect, press| face::thumbnail(page, rect, path, current, press.lit()),
        )
    }

    /// A button, as wide as its label.
    pub fn button(&mut self, label: &str) -> bool {
        let room =
            self.page.measure(Text::Label, label) + 2.0 * self.page.metric(Metric::RowPadding);
        let height = face::button_height(self.page);
        self.control(height, Some(room), self.gap, |page, rect, press| {
            // As wide as its label, unless the page is narrower.
            let label = face::fitted(page, rect, Text::Label, label);
            page.draw_button(rect, &label, press)
        })
    }

    /// Finish the frame now rather than when it is dropped.
    pub fn finish(mut self) {
        self.end();
    }

    /// A title or a heading: on as many lines as it takes to stay inside
    /// the page.
    fn line_of(&mut self, text: Text, string: &str, role: Role) {
        let line = self.page.line(text);
        let lines = self.flow.lines(self.page, string, self.rect[2], text);
        for string in lines.iter().filter(|string| !string.is_empty()) {
            if let Some(top) = self.advance(line) {
                self.page.draw_label(
                    [self.rect[0], top, self.rect[2], line],
                    text,
                    string,
                    role,
                    Align::Left,
                );
            }
        }
        self.y += self.gap;
    }

    fn paragraph(&mut self, text: &str, role: Role) {
        let line = self.page.line(Text::Caption);
        let lines = self
            .flow
            .lines(self.page, text, self.rect[2], Text::Caption);
        for string in lines.iter() {
            if string.is_empty() {
                // A blank line: half a line of air.
                self.y += line * 0.5;
                continue;
            }
            if let Some(top) = self.advance(line) {
                self.page.draw_label(
                    [self.rect[0], top, self.rect[2], line],
                    Text::Caption,
                    string,
                    role,
                    Align::Left,
                );
            }
        }
        self.y += self.gap;
    }

    /// Take `height` of the content; where it stands on screen if it is
    /// entirely in view.
    fn advance(&mut self, height: f32) -> Option<f32> {
        let top = self.rect[1] + self.y - self.flow.scroll;
        self.y += height;
        face::inside(top, height, self.clip).then_some(top)
    }

    fn control(
        &mut self,
        height: f32,
        width: Option<f32>,
        after: f32,
        draw: impl FnOnce(&Page, [f32; 4], Press),
    ) -> bool {
        self.control_shaped(height, width, after, 0.0, draw)
    }

    /// A control: in the grid's next cell while one is open, else on a line
    /// of its own; the light drawn on it as a capsule (`shape` 0) or with
    /// corners of that radius.
    fn control_shaped(
        &mut self,
        height: f32,
        width: Option<f32>,
        after: f32,
        shape: f32,
        draw: impl FnOnce(&Page, [f32; 4], Press),
    ) -> bool {
        let index = self.spans.len();
        let span = match &mut self.grid {
            Some(grid) => {
                let span = Span {
                    x: grid.column as f32 * (grid.width + grid.gap),
                    y: grid.top,
                    w: grid.width,
                    h: grid.height,
                };
                grid.column += 1;
                if grid.column >= grid.columns {
                    grid.column = 0;
                    grid.top += grid.height + grid.gap;
                }
                span
            }
            None => {
                let span = Span {
                    x: 0.0,
                    y: self.y,
                    w: width.unwrap_or(self.rect[2]).min(self.rect[2]),
                    h: height,
                };
                self.y += height + after;
                span
            }
        };
        self.spans.push(span);
        self.shapes.push(shape);
        let top = self.rect[1] + span.y - self.flow.scroll;
        let lit = self.lit && self.flow.walk.focus == Some(index);
        if face::inside(top, span.h, self.clip) {
            let rect = [self.rect[0] + span.x, top, span.w, span.h];
            if lit && !self.light_drawn {
                self.light(rect, shape);
            }
            if self.lit || self.flow.pointable {
                self.page.draw_spot(self.flow.base + index as u32, rect);
                self.spots.push(index);
            }
            let press = if lit {
                self.page.press(true)
            } else {
                Press::Resting
            };
            draw(self.page, rect, press);
        }
        if self.flow.pending == Some(index) {
            self.flow.pending = None;
            true
        } else {
            false
        }
    }

    /// Lay the light down before the controls it is behind, where the lit
    /// control stood last frame (scrolled as it is now), so every control
    /// drawn after it hands its face over to it as it arrives. While the view
    /// is still on its way to a control out of view, the light waits at the
    /// edge it will come in from.
    fn lay_the_light(&mut self) {
        if !self.lit {
            return;
        }
        let Some(span) = self
            .flow
            .walk
            .focus
            .and_then(|focus| self.flow.laid.spans.get(focus))
        else {
            return;
        };
        let rect = span.on_screen([self.rect[0], self.rect[1]], self.flow.scroll);
        let shape = self
            .flow
            .walk
            .focus
            .and_then(|focus| self.flow.shapes.get(focus))
            .copied()
            .unwrap_or(0.0);
        self.light(face::held_in(rect, self.clip), shape);
    }

    fn light(&mut self, rect: [f32; 4], shape: f32) {
        let strength = self.page.press(true).light_strength();
        let at = if shape > 0.0 {
            self.page.light_shaped(rect, shape, strength, self.jumped)
        } else if self.jumped {
            self.page.place(rect, strength)
        } else {
            self.page.glide(rect, strength)
        };
        self.page.light_at(at);
        self.light_drawn = true;
    }

    fn end(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.end_grid();
        let flow = &mut *self.flow;
        flow.shapes = std::mem::take(&mut self.shapes);
        flow.laid = Laid {
            spans: std::mem::take(&mut self.spans),
            content: self.y,
            view: self.rect[3],
        };
        flow.laid_once = true;
        flow.pinned = self.pinned;
        flow.spots = std::mem::take(&mut self.spots);
        flow.was_lit = self.lit;
        flow.taken = false;
        flow.pending = None;
        flow.pointer_pressed = false;

        let furthest = flow.laid.furthest();
        if furthest > 0.5 {
            let band = self.bleed + self.row * 0.5;
            let view = [
                self.rect[0] - self.bleed,
                self.clip[0],
                self.rect[2] + 2.0 * self.bleed,
                self.clip[1] - self.clip[0],
            ];

            let top = (flow.scroll / band).clamp(0.0, 1.0);
            let bottom = ((furthest - flow.scroll) / band).clamp(0.0, 1.0);
            self.page.draw_soft_edges(view, band, top, bottom);
        }
        if (flow.scroll - flow.walk.target).abs() > 0.5 || flow.speed.abs() > 0.5 {
            self.page.redraw_within(0.0);
        }
    }
}

impl Drop for FlowFrame<'_, '_> {
    fn drop(&mut self) {
        self.end();
    }
}
