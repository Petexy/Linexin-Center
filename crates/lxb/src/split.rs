//! The window of a multi-page application, laid out as DistriBumpy lays out
//! its own: one pane of glass over the wallpaper, the column of pages as a
//! sidebar card inside it at the left, the page beside the card, and along
//! the foot of the page the row saying what the buttons do. Every margin is
//! the same margin: the card stands one in from the left, the page ends one
//! in from the right, and between the two is the pane's own padding.
//!
//! A window too narrow for both (one standing on its side) slides between
//! them with the light, as LineXinBar's Home menu slides between its column
//! and its cards (the toolkit's `layout::beside` and `Slide`); the foot is
//! then the pane's whole width.

use crate::face;
use crate::layout::{Beside, Slide};
use crate::legend::Legend;
use crate::page::Page;
use crate::types::Metric;

/// Where a host writes the spot that lets a click on a page shown as a
/// preview move the light into it.
pub const PAGE_SPOT: u32 = 0x4C5A_0000;

/// The narrowest a page beside the column may be before the two slide, in
/// 1080-pixel reference points (DistriBumpy's `LISTING_LEAST`).
const PAGE_LEAST: f32 = 560.0;

/// Where everything stands this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Panes {
    /// The window's own pane (draw it with `Overlay::Dialog`).
    pub window: [f32; 4],
    /// The column's card (zero wide in single-page mode).
    pub column: [f32; 4],
    /// The page: everything beside the column, its foot included.
    pub page: [f32; 4],
    /// Where the page's flow goes (`LxbFrameV1::rect`): the page above its
    /// foot.
    pub content: [f32; 4],
    /// The line along the bottom the legend is written on.
    pub foot: [f32; 4],
    /// How far everything is slid to the left.
    pub slid: f32,
    /// Whether the window is too narrow for both side by side.
    pub slides: bool,
}

impl Panes {
    /// The page alone filling the window (single-widget mode).
    pub fn whole(page: &Page) -> Self {
        let (window, room) = window(page);
        let (content, foot) = above_the_foot(page, room);
        Self {
            window,
            column: [room[0], room[1], 0.0, room[3]],
            page: room,
            content,
            foot,
            slid: 0.0,
            slides: false,
        }
    }
}

/// The window's pane, and the room inside its padding.
fn window(page: &Page) -> ([f32; 4], [f32; 4]) {
    let inset = page.metric(Metric::PanelInset);
    let pad = page.metric(Metric::PanelPadding);
    let pane = [
        inset,
        inset,
        (page.width() - 2.0 * inset).max(1.0),
        (page.height() - 2.0 * inset).max(1.0),
    ];
    let room = [
        pane[0] + pad,
        pane[1] + pad,
        (pane[2] - 2.0 * pad).max(1.0),
        (pane[3] - 2.0 * pad).max(1.0),
    ];
    (pane, room)
}

/// `area` split into what stands above the legend's line, and that line.
///
/// A flow draws what scrolls away a pane's padding beyond its rectangle
/// (and softens it there), so the content stops that far above the foot:
/// nothing on its way out runs into, or blurs, the legend.
fn above_the_foot(page: &Page, area: [f32; 4]) -> ([f32; 4], [f32; 4]) {
    let tall = Legend::height(page);
    let gap = page.metric(Metric::Gap);
    let bleed = page.metric(Metric::PanelPadding);
    let bottom = area[1] + area[3];
    let foot = [area[0], bottom - tall, area[2], tall];
    let content = [
        area[0],
        area[1],
        area[2],
        (foot[1] - gap - bleed - area[1]).max(0.0),
    ];
    (content, foot)
}

/// The column and its page: side by side where the window is wide enough,
/// otherwise one view sliding left while the page has the light. Keep one
/// across frames.
#[derive(Debug, Clone, Default)]
pub struct Split {
    slide: Slide,
    size: [f32; 2],
    column: Option<f32>,
    seconds: Option<f32>,
}

impl Split {
    pub fn new() -> Self {
        Self::default()
    }

    /// How wide the column's card is, in window pixels (by default
    /// DistriBumpy's 348 reference points); see
    /// [`Column::fitting_width`](crate::Column::fitting_width).
    pub fn set_column_width(&mut self, width: Option<f32>) {
        self.column = width;
    }

    /// Lay everything out for this frame, sliding towards the page while
    /// `page_has_the_light`.
    pub fn layout(&mut self, page: &Page, page_has_the_light: bool) -> Panes {
        let dt = face::delta(&mut self.seconds, page.seconds());
        let size = [page.width(), page.height()];
        if size != self.size {
            // A slide out of a layout no longer on screen would be a lie.
            self.slide.place_again();
            self.size = size;
        }
        let (window, room) = window(page);
        let gap = page.metric(Metric::PanelPadding);
        let full = self.column.unwrap_or_else(|| page.scaled(348.0));
        let least = page.scaled(PAGE_LEAST);
        // Held to a third of the room while the page keeps its least width
        // beside it; otherwise the card keeps its width and the two slide.
        let capped = full.min(room[2] * 0.32).max(page.scaled(150.0));
        if room[2] - capped - gap >= least {
            let column = [room[0], room[1], capped, room[3]];
            let beside = [
                room[0] + capped + gap,
                room[1],
                (room[2] - capped - gap).max(0.0),
                room[3],
            ];
            let (content, foot) = above_the_foot(page, beside);
            return Panes {
                window,
                column,
                page: beside,
                content,
                foot,
                slid: 0.0,
                slides: false,
            };
        }
        let laid = Beside::new(room[2], 0.0, full, gap, least);
        let slid = if page.low_end() {
            let target = laid.target(page_has_the_light);
            self.slide.follow(target, 1.0);
            target
        } else {
            self.slide.follow(laid.target(page_has_the_light), dt)
        };
        if self.slide.moving() {
            page.redraw_within(0.0);
        }
        // The legend's line runs the whole room; both stop above it.
        let (above, foot) = above_the_foot(page, room);
        let column = [room[0] - slid, room[1], full, above[3]];
        let beside = [room[0] + laid.page_x - slid, room[1], laid.page_w, room[3]];
        let content = [beside[0], above[1], beside[2], above[3]];
        Panes {
            window,
            column,
            page: beside,
            content,
            foot,
            slid,
            slides: true,
        }
    }
}
