//! Motion and the column-beside-a-page layout, from `liblxb_toolkit` where it
//! is installed, and from a transcription of the same arithmetic where it is
//! not (so the pure logic of this crate also runs without LineXinBar).

use crate::sys::{RawBeside, RawSlide, load};

/// The highlight's spring rate (`motion::HIGHLIGHT_SPRING`).
const HIGHLIGHT_SPRING: f64 = 21.0;
/// The cards' spring rate (`motion::CARD_SPRING`).
const CARD_SPRING: f64 = 19.0;
/// `layout::PEEK`.
const PEEK: f32 = 0.12;
/// `layout::AT_REST`.
const AT_REST: f32 = 0.5;

/// `motion::spring`: one step of a critically damped spring.
fn spring(position: f64, velocity: f64, target: f64, rate: f64, dt: f64) -> (f64, f64) {
    let dt = dt.clamp(0.0, 0.1);
    let offset = position - target;
    let c = velocity + rate * offset;
    let decay = (-rate * dt).exp();
    (
        target + (offset + c * dt) * decay,
        (velocity - c * rate * dt) * decay,
    )
}

/// Move `position` (with `velocity`) `dt` seconds towards `target` on the
/// light's own spring (`lxb_glide`), which is also what a scrolled list rides.
pub fn glide(position: &mut f32, velocity: &mut f32, target: f32, dt: f32) {
    if let Ok(lxb) = load()
        && let Some(function) = lxb.sym.lxb_glide
    {
        // SAFETY: two valid, exclusive pointers.
        unsafe { function(position, velocity, target, dt) };
        return;
    }
    let (at, speed) = spring(
        f64::from(*position),
        f64::from(*velocity),
        f64::from(target),
        HIGHLIGHT_SPRING,
        f64::from(dt),
    );
    *position = at as f32;
    *velocity = speed as f32;
}

/// How much of the window the column keeps showing while the page has the
/// light on a window too narrow for both (`lxb_peek`).
pub fn peek() -> f32 {
    load()
        .ok()
        .and_then(|lxb| lxb.sym.lxb_peek)
        // SAFETY: takes nothing, answers a constant.
        .map_or(PEEK, |function| unsafe { function() })
}

/// Where the page beside a column stands (`lxb_beside`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beside {
    /// Where the page starts before the view slides.
    pub page_x: f32,
    /// How wide the page is laid out.
    pub page_w: f32,
    /// How far the view slides left while the page has the light; nought
    /// where the two stand side by side.
    pub reach: f32,
}

impl Beside {
    /// The column's window is `window` wide; the column is `column` wide and
    /// `margin` in from the left; the page starts `gap` beyond it and keeps
    /// to the two side by side while it has at least `least` (`lxb_layout_beside`).
    pub fn new(window: f32, margin: f32, column: f32, gap: f32, least: f32) -> Self {
        if let Ok(lxb) = load()
            && let Some(function) = lxb.sym.lxb_layout_beside
        {
            // SAFETY: plain values in, a plain value out.
            let RawBeside {
                page_x,
                page_w,
                reach,
            } = unsafe { function(window, margin, column, gap, least) };
            return Self {
                page_x,
                page_w,
                reach,
            };
        }
        Self::transcribed(window, margin, column, gap, least)
    }

    /// `layout::beside`, for a machine without the toolkit.
    fn transcribed(window: f32, margin: f32, column: f32, gap: f32, least: f32) -> Self {
        let page_x = margin + column + gap;
        let room = (window - page_x - margin).max(0.0);
        if room >= least {
            return Self {
                page_x,
                page_w: room,
                reach: 0.0,
            };
        }
        let page_w = (window - margin * 2.0 - window * PEEK).max(room);
        Self {
            page_x,
            page_w,
            reach: (page_x + page_w + margin - window).max(0.0),
        }
    }

    /// Whether the two do not fit side by side, so the view slides.
    pub fn slides(&self) -> bool {
        self.reach > 0.0
    }

    /// Where the view is going.
    pub fn target(&self, page_has_the_light: bool) -> f32 {
        if page_has_the_light { self.reach } else { 0.0 }
    }
}

/// The view's slide between a column and its page, on the cards' spring
/// (`lxb_slide`). Keep one, and follow every frame; the first frame stands
/// where it is asked to without sliding.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Slide {
    raw: RawSlide,
}

impl Slide {
    /// Move `dt` seconds towards `target`; where the view stands now.
    pub fn follow(&mut self, target: f32, dt: f32) -> f32 {
        if let Ok(lxb) = load()
            && let Some(function) = lxb.sym.lxb_slide_follow
        {
            // SAFETY: a valid, exclusive `lxb_slide`.
            return unsafe { function(&mut self.raw, target, dt) };
        }
        let raw = &mut self.raw;
        raw.target = target;
        if raw.placed == 0 {
            raw.placed = 1;
            raw.at = target;
            raw.speed = 0.0;
            return raw.at;
        }
        let (at, speed) = spring(
            f64::from(raw.at),
            f64::from(raw.speed),
            f64::from(target),
            CARD_SPRING,
            f64::from(dt),
        );
        raw.at = at as f32;
        raw.speed = speed as f32;
        if (raw.at - target).abs() < AT_REST && raw.speed.abs() < AT_REST {
            raw.at = target;
            raw.speed = 0.0;
        }
        raw.at
    }

    /// Whether the view is still on its way.
    pub fn moving(&self) -> bool {
        if let Ok(lxb) = load()
            && let Some(function) = lxb.sym.lxb_slide_moving
        {
            // SAFETY: a valid `lxb_slide`.
            return unsafe { function(&self.raw) } != 0;
        }
        self.raw.placed != 0 && (self.raw.at != self.raw.target || self.raw.speed != 0.0)
    }

    /// Where the view stands.
    pub fn at(&self) -> f32 {
        self.raw.at
    }

    /// Start again: the next follow stands where it is asked to (a window
    /// whose column and page changed width must not slide out of a layout
    /// that is no longer on the screen).
    pub fn place_again(&mut self) {
        self.raw = RawSlide::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transcription_keeps_a_wide_window_side_by_side() {
        let wide = Beside::transcribed(1920.0, 24.0, 300.0, 18.0, 600.0);
        assert_eq!(wide.page_x, 342.0);
        assert_eq!(wide.page_w, 1920.0 - 342.0 - 24.0);
        assert!(!wide.slides());
        assert_eq!(wide.target(true), 0.0);
    }

    #[test]
    fn the_transcription_slides_a_narrow_window_to_a_page_it_can_read() {
        let (window, margin, column, gap) = (620.0, 16.0, 380.0, 12.0);
        let narrow = Beside::transcribed(window, margin, column, gap, 400.0);
        assert!(narrow.slides());
        assert!((narrow.page_w - (window - margin * 2.0 - window * PEEK)).abs() < 1e-3);
        // Slid by `reach`, the page ends a margin from the right-hand edge.
        let right = narrow.page_x - narrow.reach + narrow.page_w;
        assert!((right - (window - margin)).abs() < 1e-3);
    }

    #[test]
    fn a_spring_arrives_without_crossing() {
        let (mut at, mut speed) = (0.0f64, 0.0f64);
        for _ in 0..240 {
            (at, speed) = spring(at, speed, 100.0, HIGHLIGHT_SPRING, 1.0 / 60.0);
            assert!(at <= 100.0 + 1e-6);
        }
        assert!((at - 100.0).abs() < 0.01);
    }
}
