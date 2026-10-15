//! A panel beside a page in a wide window, as the GTK pages put one there
//! (Linexin Hello's New and Quick Links, the package manager's actions):
//! two flows side by side, each scrolling on its own, one light between them.
//! Right past the page's last column carries the light into the panel; Left
//! past the panel's first, or Back, brings it back; the pointer moving onto
//! either takes the light there. In a narrow window there is no panel and
//! the page is one flow (the widget draws the panel's controls into it).

use crate::flow::{FLOW_SPOTS, Flow, Input, gallery_strip};
use crate::page::Page;
use crate::types::{Action, Metric};

/// Where the panel's flow numbers its pointer spots from.
pub const PANEL_SPOTS: u32 = FLOW_SPOTS + 0x8000;

/// A page and the panel beside it, kept across frames.
#[derive(Debug, Clone)]
pub struct SidePanel {
    pub main: Flow,
    pub side: Flow,
    /// The light is in the panel.
    in_side: bool,
}

impl Default for SidePanel {
    fn default() -> Self {
        Self::new()
    }
}

impl SidePanel {
    pub fn new() -> Self {
        Self {
            main: Flow::new(),
            side: Flow::new().spots_from(PANEL_SPOTS),
            in_side: false,
        }
    }

    /// How wide a panel of rows and cards is beside a page `rect` wide: a
    /// third of it, room enough for a row's mark and a few words.
    pub fn width(page: &Page, rect: [f32; 4]) -> f32 {
        (rect[2] * 0.32).clamp(page.scaled(420.0), page.scaled(500.0))
    }

    /// How wide a panel holding a gallery of 16:9 pictures is beside a page
    /// `rect`: `share` of the page, or less where the window is too short
    /// for a picture that wide (under a heading `pinned` tall, over the
    /// thumbnails), so the picture fills the panel and the page takes the
    /// rest rather than leaving air on both sides of the picture.
    pub fn gallery_width(page: &Page, rect: [f32; 4], share: f32, pinned: f32) -> f32 {
        let strip = gallery_strip(page.metric(Metric::RowHeight));
        let room = rect[3] - pinned - strip - page.metric(Metric::Gap) * 4.0;
        (rect[2] * share).min(room.max(page.scaled(160.0)) * 16.0 / 9.0)
    }

    /// The air between the page and its panel: the page's own margin, the
    /// air between the Center's column and the page, so the page stands
    /// between the two evenly rather than leaving a hole beside the panel.
    pub fn spacing(page: &Page) -> f32 {
        page.metric(Metric::PanelPadding)
    }

    /// Where the page and its panel stand in `rect`: the panel `width` wide
    /// at the right, while the page keeps at least `least` beside it;
    /// otherwise no panel.
    pub fn split(
        page: &Page,
        rect: [f32; 4],
        width: f32,
        least: f32,
    ) -> ([f32; 4], Option<[f32; 4]>) {
        let spacing = Self::spacing(page);
        let main = rect[2] - width - spacing;
        if main < least {
            return (rect, None);
        }
        (
            [rect[0], rect[1], main, rect[3]],
            Some([rect[0] + main + spacing, rect[1], width, rect[3]]),
        )
    }

    /// This frame's actions for the page and the panel (`focused`: the
    /// widget's page has the light; `side`: a panel is shown).
    pub fn input(
        &mut self,
        page: &Page,
        actions: &[Action],
        focused: bool,
        side: bool,
    ) -> (Input, Input) {
        if !side {
            self.in_side = false;
        }
        // The flow without the light still answers the pointer, to take it.
        self.main.set_pointable(focused && side && self.in_side);
        self.side.set_pointable(focused && side && !self.in_side);
        if !focused {
            return Default::default();
        }
        if self.in_side {
            (Input::default(), self.side.input(page, actions))
        } else {
            (self.main.input(page, actions), Input::default())
        }
    }

    /// Whether the page and the panel are lit this frame.
    pub fn lit(&self, focused: bool, side: bool) -> (bool, bool) {
        let in_side = side && self.in_side;
        (focused && !in_side, focused && in_side)
    }

    /// After both are drawn: the light crosses where it was asked to, or
    /// where the pointer took it. Whether the page gives the light back
    /// (Back, or Left past the page's first column).
    pub fn settle(&mut self, main: &Input, side: &Input, has_side: bool) -> bool {
        if !has_side {
            return main.leave.is_some();
        }
        if self.in_side {
            if side.leave.is_some() {
                self.in_side = false;
            }
        } else if main.unhandled.contains(&Action::Right) {
            self.in_side = true;
        }
        if self.side.claimed() {
            self.in_side = true;
        } else if self.main.claimed() {
            self.in_side = false;
        }
        main.leave.is_some()
    }

    /// Whether the light is in the panel.
    pub fn in_side(&self) -> bool {
        self.in_side
    }
}
