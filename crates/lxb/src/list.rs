//! The arithmetic of a list that scrolls to keep its light in view, with no
//! library behind it: shared by [`Flow`](crate::Flow) and
//! [`Column`](crate::Column), and tested on its own.

/// Where one control of a list stood, in the list's own coordinates: from
/// the content's left edge and top, before any scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Span {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Span {
    /// Where it stands on screen with the content's corner at `origin`,
    /// scrolled by `scroll`.
    pub fn on_screen(&self, origin: [f32; 2], scroll: f32) -> [f32; 4] {
        [
            origin[0] + self.x,
            origin[1] + self.y - scroll,
            self.w,
            self.h,
        ]
    }

    /// Whether all of it is inside a view `view` tall scrolled by `scroll`.
    pub fn inside(&self, scroll: f32, view: f32) -> bool {
        self.y >= scroll - 0.5 && self.y + self.h <= scroll + view + 0.5
    }
}

/// A list as it was laid out on the last frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Laid {
    /// Every control, in order.
    pub spans: Vec<Span>,
    /// How tall everything was.
    pub content: f32,
    /// How tall the view onto it was.
    pub view: f32,
}

impl Laid {
    pub fn count(&self) -> usize {
        self.spans.len()
    }

    /// How far the view can scroll.
    pub fn furthest(&self) -> f32 {
        (self.content - self.view).max(0.0)
    }

    /// Whether everything fits without scrolling.
    #[cfg(test)]
    pub fn fits(&self) -> bool {
        self.furthest() <= 0.5
    }

    /// Where the view should stand for the control at `focus` to be in view
    /// with `margin` of air: it moves only as far as it has to, back to the
    /// top for the first control and down to the end for the last when that
    /// keeps them in view.
    pub fn follow(&self, focus: usize, current: f32, margin: f32) -> f32 {
        let furthest = self.furthest();
        let Some(span) = self.spans.get(focus) else {
            return current.clamp(0.0, furthest);
        };
        let margin = margin.min(((self.view - span.h) * 0.5).max(0.0));
        let mut target = current;
        if span.y - margin < target {
            target = span.y - margin;
        }
        if span.y + span.h + margin > target + self.view {
            target = span.y + span.h + margin - self.view;
        }
        if focus == 0 && span.y + span.h + margin <= self.view {
            target = 0.0;
        }
        if focus + 1 == self.spans.len() && self.content - (span.y - margin) <= self.view {
            target = furthest;
        }
        target.clamp(0.0, furthest)
    }

    /// The control under (`down`) or over the one at `focus`: the nearest
    /// in the rows that way, among those standing across from it (the same
    /// column of a grid) where there are any, else the nearest at all.
    pub fn beyond(&self, focus: usize, down: bool) -> Option<usize> {
        let from = *self.spans.get(focus)?;
        let centre = from.x + from.w * 0.5;
        self.spans
            .iter()
            .enumerate()
            .filter(|(index, span)| {
                *index != focus
                    && if down {
                        span.y >= from.y + from.h - 0.5
                    } else {
                        span.y + span.h <= from.y + 0.5
                    }
            })
            .min_by(|(_, a), (_, b)| {
                let key = |span: &Span| {
                    let across = span.x < from.x + from.w - 0.5 && span.x + span.w > from.x + 0.5;
                    let away = if down {
                        span.y - from.y
                    } else {
                        from.y - span.y
                    };
                    // From something wider than it (a row over a grid), the
                    // first of a row in reading order; else the nearest.
                    let aside = if from.w > span.w * 1.5 {
                        (span.x - from.x).abs()
                    } else {
                        (span.x + span.w * 0.5 - centre).abs()
                    };
                    (!across, away, aside)
                };
                let (a, b) = (key(a), key(b));
                a.0.cmp(&b.0)
                    .then(a.1.total_cmp(&b.1))
                    .then(a.2.total_cmp(&b.2))
            })
            .map(|(index, _)| index)
    }

    /// The control beside the one at `focus` in its row, to the right
    /// (`right`) or the left: the nearest that stands level with it.
    pub fn beside(&self, focus: usize, right: bool) -> Option<usize> {
        let from = *self.spans.get(focus)?;
        self.spans
            .iter()
            .enumerate()
            .filter(|(index, span)| {
                *index != focus
                    && span.y < from.y + from.h - 0.5
                    && span.y + span.h > from.y + 0.5
                    && if right {
                        span.x >= from.x + from.w - 0.5
                    } else {
                        span.x + span.w <= from.x + 0.5
                    }
            })
            .min_by(|(_, a), (_, b)| {
                let away = |span: &Span| {
                    if right {
                        span.x - from.x
                    } else {
                        from.x - span.x
                    }
                };
                away(a).total_cmp(&away(b))
            })
            .map(|(index, _)| index)
    }

    /// The first control entirely in view.
    pub fn first_inside(&self, scroll: f32) -> Option<usize> {
        self.spans
            .iter()
            .position(|span| span.inside(scroll, self.view))
    }
}

/// What a step of the light did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// The light moved to another control.
    Moved,
    /// No control to move to, but the view moved.
    Scrolled,
    /// Nothing moved: the light is at the end and so is the view.
    Stuck,
}

/// Which control the light comes back to after it stepped off the end of the
/// controls to read what lies past them, and on which side of the view it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resume {
    /// The control is below what is being read; Down brings the light back.
    Below(usize),
    /// The control is above; Up brings it back.
    Above(usize),
}

/// Where the light of a list is, and where the view onto it is going.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Walk {
    /// The control the light is on; `None` while the page is being read
    /// rather than chosen from (no controls, or past the end of them).
    pub focus: Option<usize>,
    pub resume: Option<Resume>,
    /// Where the view is going (it glides there).
    pub target: f32,
}

impl Walk {
    /// One step down (`down`) or up. `step` is how far the view moves when
    /// there is no control to move to.
    pub fn step(&mut self, down: bool, laid: &Laid, step: f32) -> Step {
        let count = laid.count();
        let furthest = laid.furthest();
        match self.focus {
            Some(focus) if focus < count => {
                if let Some(next) = laid.beyond(focus, down) {
                    self.focus = Some(next);
                    Step::Moved
                } else if down && self.target < furthest - 0.5 {
                    self.focus = None;
                    self.resume = Some(Resume::Above(focus));
                    self.target = (self.target + step).min(furthest);
                    Step::Scrolled
                } else if !down && self.target > 0.5 {
                    self.focus = None;
                    self.resume = Some(Resume::Below(focus));
                    self.target = (self.target - step).max(0.0);
                    Step::Scrolled
                } else {
                    Step::Stuck
                }
            }
            _ => {
                if let Some(resume) = self.resume {
                    let back = match resume {
                        Resume::Below(index) if down => Some(index),
                        Resume::Above(index) if !down => Some(index),
                        _ => None,
                    };
                    if let Some(index) = back.filter(|index| *index < count)
                        && laid.spans[index].inside(self.target, laid.view)
                    {
                        self.focus = Some(index);
                        self.resume = None;
                        return Step::Moved;
                    }
                }
                let before = self.target;
                self.target = if down {
                    (self.target + step).min(furthest)
                } else {
                    (self.target - step).max(0.0)
                };
                if (self.target - before).abs() > 0.01 {
                    Step::Scrolled
                } else {
                    Step::Stuck
                }
            }
        }
    }

    /// The light arriving on the list (it had been elsewhere): it stays on
    /// its control if that is in view, otherwise it lands on the first
    /// control in view, otherwise it waits for one to scroll in.
    pub fn arrive(&mut self, laid: &Laid, scroll: f32) {
        if let Some(focus) = self.focus
            && laid
                .spans
                .get(focus)
                .is_some_and(|span| span.inside(scroll, laid.view))
        {
            return;
        }
        self.target = scroll;
        self.focus = laid.first_inside(scroll);
        self.resume = None;
        if self.focus.is_none() && laid.count() > 0 {
            self.resume = Some(match laid.spans.iter().position(|span| span.y >= scroll) {
                Some(below) => Resume::Below(below),
                None => Resume::Above(laid.count() - 1),
            });
        }
    }

    /// One step to the right (`right`) or left, to the control beside the
    /// light's in its row (a grid's next card); whether it moved.
    pub fn side(&mut self, right: bool, laid: &Laid) -> bool {
        match self.focus.and_then(|focus| laid.beside(focus, right)) {
            Some(next) => {
                self.focus = Some(next);
                true
            }
            None => false,
        }
    }

    /// Put the light on `index` (a click, a hover, the program).
    pub fn put(&mut self, index: usize) {
        self.focus = Some(index);
        self.resume = None;
    }

    /// Keep the light on a control that still exists, and the view where it
    /// has to be for it.
    pub fn settle(&mut self, laid: &Laid, margin: f32) {
        let count = laid.count();
        if let Some(focus) = self.focus
            && focus >= count
        {
            self.focus = count.checked_sub(1);
        }
        self.target = match self.focus {
            Some(focus) => laid.follow(focus, self.target, margin),
            None => self.target.clamp(0.0, laid.furthest()),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `count` rows `row` tall, one under the other, below `above` of text.
    fn rows(count: usize, row: f32, above: f32, view: f32) -> Laid {
        Laid {
            spans: (0..count)
                .map(|index| Span {
                    x: 0.0,
                    y: above + index as f32 * row,
                    w: 300.0,
                    h: row,
                })
                .collect(),
            content: above + count as f32 * row,
            view,
        }
    }

    #[test]
    fn a_list_that_fits_never_scrolls() {
        let laid = rows(5, 50.0, 0.0, 400.0);
        assert!(laid.fits());
        for focus in 0..5 {
            assert_eq!(laid.follow(focus, 0.0, 50.0), 0.0);
        }
    }

    #[test]
    fn the_view_moves_only_as_far_as_the_light_needs() {
        let laid = rows(30, 50.0, 0.0, 400.0);
        // In view already: nothing moves.
        assert_eq!(laid.follow(3, 0.0, 50.0), 0.0);
        // Row 9 ends at 500: the view goes just far enough to show a row of
        // air under it.
        assert_eq!(laid.follow(9, 0.0, 50.0), 500.0 + 50.0 - 400.0);
        // Coming back up, the row stands a margin under the top.
        assert_eq!(laid.follow(4, 300.0, 50.0), 150.0);
        // The ends go all the way.
        assert_eq!(laid.follow(0, 300.0, 50.0), 0.0);
        assert_eq!(laid.follow(29, 300.0, 50.0), laid.furthest());
    }

    #[test]
    fn the_margin_never_pushes_a_tall_control_out_of_view() {
        let laid = rows(3, 380.0, 0.0, 400.0);
        let target = laid.follow(1, 0.0, 100.0);
        let span = laid.spans[1];
        assert!(span.inside(target, laid.view), "{target}");
    }

    #[test]
    fn walking_moves_the_light_and_stops_at_the_ends() {
        let laid = rows(3, 50.0, 0.0, 400.0);
        let mut walk = Walk {
            focus: Some(0),
            ..Walk::default()
        };
        assert_eq!(walk.step(false, &laid, 50.0), Step::Stuck);
        assert_eq!(walk.step(true, &laid, 50.0), Step::Moved);
        assert_eq!(walk.step(true, &laid, 50.0), Step::Moved);
        assert_eq!(walk.focus, Some(2));
        assert_eq!(walk.step(true, &laid, 50.0), Step::Stuck);
        assert_eq!(walk.focus, Some(2));
    }

    #[test]
    fn the_light_steps_off_the_first_control_to_read_what_is_above_it() {
        // 600 of text, then rows: the first row is out of a 400 view at the top.
        let laid = rows(4, 50.0, 600.0, 400.0);
        let mut walk = Walk {
            focus: Some(0),
            ..Walk::default()
        };
        walk.settle(&laid, 50.0);
        assert!(walk.target > 0.0);
        let shown = walk.target;
        // Up: no control above, so the light lets go and the text scrolls in.
        assert_eq!(walk.step(false, &laid, 76.0), Step::Scrolled);
        assert_eq!(walk.focus, None);
        assert_eq!(walk.target, shown - 76.0);
        walk.settle(&laid, 50.0);
        assert_eq!(walk.target, shown - 76.0, "reading is not pulled back");
        // Down scrolls back until the row is whole, and only then lights it.
        let mut steps = 0;
        while walk.step(true, &laid, 76.0) == Step::Scrolled {
            steps += 1;
        }
        assert_eq!(walk.focus, Some(0));
        assert!(steps <= 1, "{steps}");
    }

    /// A grid of `columns` cards `w` wide and 80 tall, `count` of them,
    /// under a full-width row.
    fn grid(count: usize, columns: usize) -> Laid {
        let mut spans = vec![Span {
            x: 0.0,
            y: 0.0,
            w: 600.0,
            h: 50.0,
        }];
        spans.extend((0..count).map(|index| Span {
            x: (index % columns) as f32 * 200.0,
            y: 60.0 + (index / columns) as f32 * 90.0,
            w: 190.0,
            h: 80.0,
        }));
        Laid {
            spans,
            content: 1000.0,
            view: 400.0,
        }
    }

    #[test]
    fn a_grid_is_walked_across_and_down_its_columns() {
        // The row (0), then cards 1 2 3 / 4 5 6 / 7.
        let laid = grid(7, 3);
        let mut walk = Walk {
            focus: Some(0),
            ..Walk::default()
        };
        assert_eq!(walk.step(true, &laid, 50.0), Step::Moved);
        assert_eq!(walk.focus, Some(1), "the row's nearest card under it");
        assert!(walk.side(true, &laid));
        assert!(walk.side(true, &laid));
        assert_eq!(walk.focus, Some(3));
        assert!(!walk.side(true, &laid), "nothing right of the last column");
        assert_eq!(walk.step(true, &laid, 50.0), Step::Moved);
        assert_eq!(walk.focus, Some(6), "the same column, a row down");
        assert_eq!(walk.step(true, &laid, 50.0), Step::Moved);
        assert_eq!(walk.focus, Some(7), "the short last row's nearest card");
        assert!(!walk.side(false, &laid));
        assert_eq!(walk.step(false, &laid, 50.0), Step::Moved);
        assert_eq!(walk.focus, Some(4));
        assert!(walk.side(true, &laid));
        assert_eq!(walk.focus, Some(5));
        assert_eq!(walk.step(false, &laid, 50.0), Step::Moved);
        assert_eq!(walk.focus, Some(2));
        assert_eq!(walk.step(false, &laid, 50.0), Step::Moved);
        assert_eq!(walk.focus, Some(0));
    }

    #[test]
    fn a_page_without_controls_scrolls_and_stops() {
        let laid = Laid {
            spans: Vec::new(),
            content: 1000.0,
            view: 400.0,
        };
        let mut walk = Walk::default();
        assert_eq!(walk.step(false, &laid, 100.0), Step::Stuck);
        for _ in 0..6 {
            assert_eq!(walk.step(true, &laid, 100.0), Step::Scrolled);
        }
        assert_eq!(walk.target, 600.0);
        assert_eq!(walk.step(true, &laid, 100.0), Step::Stuck);
    }

    #[test]
    fn arriving_keeps_a_light_in_view_or_lands_on_the_first_one_shown() {
        let laid = rows(30, 50.0, 0.0, 400.0);
        let mut walk = Walk {
            focus: Some(3),
            ..Walk::default()
        };
        walk.arrive(&laid, 0.0);
        assert_eq!(walk.focus, Some(3));
        walk.arrive(&laid, 520.0);
        assert_eq!(walk.focus, Some(11), "the first row wholly below 520");
        // Text only in view: the light waits below for the first row.
        let intro = rows(2, 50.0, 900.0, 400.0);
        let mut walk = Walk::default();
        walk.arrive(&intro, 0.0);
        assert_eq!(walk.focus, None);
        assert_eq!(walk.resume, Some(Resume::Below(0)));
    }

    #[test]
    fn a_light_past_a_shrunken_list_comes_back_onto_it() {
        let laid = rows(2, 50.0, 0.0, 400.0);
        let mut walk = Walk {
            focus: Some(7),
            target: 300.0,
            ..Walk::default()
        };
        walk.settle(&laid, 50.0);
        assert_eq!(walk.focus, Some(1));
        assert_eq!(walk.target, 0.0);
        let mut walk = Walk {
            focus: Some(0),
            ..Walk::default()
        };
        walk.settle(&Laid::default(), 50.0);
        assert_eq!(walk.focus, None);
    }
}
