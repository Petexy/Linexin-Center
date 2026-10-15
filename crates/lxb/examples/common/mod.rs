//! A small host in the shape of the Center's LineXinBar front end: a column
//! of pages beside the chosen page, the light moving between them. Shared by
//! `examples/demo.rs` and the integration tests' pictures.

#![allow(dead_code)]

use linexin_lxb::{
    Action, Button, Column, ColumnContent, ColumnEntry, Flow, Hands, Hint, Legend, Overlay,
    PAGE_SPOT, Page, Panes, Split, WidgetFrame, guard, hand_back_sound, host_frame, page_result,
    raw_actions,
};
use linexin_plugin_api::{LXB_PAGE_LEAVE, LXB_PAGE_STAY, LxbFrameV1};
use std::ffi::c_void;

/// What a demo page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Names and values, a note, two buttons.
    System,
    /// A long list of settings, scrolled.
    Settings,
    /// Text above and below a handful of controls.
    Reading,
    /// Nothing to choose: text only.
    About,
}

/// One page, as a widget keeps it.
pub struct DemoPage {
    pub mark: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    pub flow: Flow,
    pub pressed: Option<String>,
    /// A panel to raise on the next frame the page has the light.
    pub raise: Option<Raise>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raise {
    Menu,
    Dialog,
}

/// The whole screen.
pub struct Demo {
    pub column: Column,
    pub split: Split,
    pub pages: Vec<DemoPage>,
    pub page_lit: bool,
    pub single: bool,
    asked: bool,
    hands: Hands,
    legend: Legend,
}

impl Demo {
    pub fn new() -> Self {
        let page = |mark, name, kind| DemoPage {
            mark,
            name,
            kind,
            flow: Flow::new(),
            pressed: None,
            raise: None,
        };
        Self {
            column: Column::new(),
            split: Split::new(),
            pages: vec![
                page("setting-info", "System Information", Kind::System),
                page("setting-display", "Display", Kind::Settings),
                page("setting-network", "Network", Kind::Reading),
                page("setting-bluetooth", "Bluetooth", Kind::Settings),
                page("setting-appearance", "Appearance", Kind::Reading),
                page("logo", "About", Kind::About),
            ],
            page_lit: false,
            single: false,
            asked: false,
            hands: Hands::new(),
            legend: Legend::new(),
        }
    }

    /// One frame of the screen.
    pub fn frame(&mut self, page: &Page) {
        let mut actions = page.actions();
        self.hands.follow(page, &actions);
        if let Some(button) = self.legend.pressed(page) {
            self.hands.clicked();
            actions.push(button.action());
        }
        if self.asked
            && let Some(answer) = page.answered()
        {
            self.asked = false;
            if answer == 0 {
                page.quit();
            }
        }

        let entries: Vec<ColumnEntry> = self
            .pages
            .iter()
            .map(|demo| ColumnEntry {
                mark: demo.mark,
                label: demo.name,
                value: (demo.kind == Kind::Settings).then_some("12"),
                ..ColumnEntry::default()
            })
            .collect();
        let column_has_it = !self.page_lit && !self.single;
        let taken = self.column.input(
            page,
            if column_has_it { &actions } else { &[] },
            entries.len(),
            column_has_it,
        );
        let mut for_page = if column_has_it {
            taken.for_page.clone()
        } else {
            actions.clone()
        };
        if taken.reclaimed {
            self.page_lit = false;
            for_page.clear();
        }
        if taken.enter {
            self.page_lit = true;
        }
        if taken.back {
            self.ask_to_leave(page);
        }
        if !self.page_lit && page.pressed(PAGE_SPOT) {
            self.page_lit = true;
        }

        let content = ColumnContent {
            entries: &entries,
            ..ColumnContent::default()
        };
        let panes = if self.single {
            Panes::whole(page)
        } else {
            self.split
                .set_column_width(Some(Column::fitting_width(page, &content)));
            self.split.layout(page, self.page_lit)
        };
        page.draw_pane(panes.window, Overlay::Dialog);
        if !self.single {
            self.column
                .draw(page, panes.column, &content, !self.page_lit);
        }
        let lit = self.page_lit || self.single;
        if !lit {
            page.draw_spot(PAGE_SPOT, panes.page);
        }

        // Through the real ABI: the frame a widget's `lxb_page` receives.
        let raw = raw_actions(if lit { &for_page } else { &[] });
        let frame = host_frame(page, panes.content, lit, &raw);
        let chosen = &mut self.pages[self.column.selected()];
        // SAFETY: the state is alive and the frame valid for the call.
        let result = unsafe { demo_page((chosen as *mut DemoPage).cast(), &frame) };
        if result == LXB_PAGE_LEAVE {
            if self.single {
                self.ask_to_leave(page);
            } else {
                self.page_lit = false;
                page.play(hand_back_sound(&for_page));
            }
        }

        if self.hands.shown() {
            let hints = if self.single {
                vec![Hint::new("Close", Button::Back)]
            } else if self.page_lit {
                vec![Hint::new("Back", Button::Back)]
            } else {
                vec![Hint::new("Open", Button::Accept)]
            };
            let foot = panes.foot;
            self.legend.draw(
                page,
                foot[0] + foot[2],
                foot[1] + foot[3] * 0.5,
                &hints,
                self.hands.pad(),
            );
        } else {
            self.legend.clear();
        }
    }

    fn ask_to_leave(&mut self, page: &Page) {
        page.ask(
            "Leave Linexin Center?",
            "The window closes. Anything a page is still doing carries on until it is done.",
            &["Leave", "Stay"],
        );
        self.asked = true;
    }
}

/// A widget's `lxb_page`, as a native widget writes one.
unsafe extern "C" fn demo_page(state: *mut c_void, frame: *const LxbFrameV1) -> u32 {
    guard(LXB_PAGE_STAY, || {
        // SAFETY: the host's state for this page, alive for the call.
        let Some(state) = (unsafe { state.cast::<DemoPage>().as_mut() }) else {
            return LXB_PAGE_STAY;
        };
        // SAFETY: the host's frame, read for this call only.
        let Some(frame) = (unsafe { WidgetFrame::read(frame) }) else {
            return LXB_PAGE_STAY;
        };
        state.draw(&frame)
    })
}

impl DemoPage {
    fn draw(&mut self, frame: &WidgetFrame) -> u32 {
        let page = &frame.page;
        let input = if frame.focused {
            self.flow.input(page, &frame.actions)
        } else {
            Default::default()
        };
        let mut menu = input.unhandled.contains(&Action::Menu);
        if frame.focused
            && let Some(raise) = self.raise.take()
        {
            match raise {
                Raise::Menu => menu = true,
                Raise::Dialog => page.ask(
                    "Forget the stored password?",
                    "Privileged commands will ask for it again.",
                    &["Forget", "Keep"],
                ),
            }
        }
        if let Some(chosen) = page.chose() {
            self.pressed = Some(format!("menu {chosen}"));
        }

        let mut f = self.flow.begin(page, frame.rect, frame.focused);
        match self.kind {
            Kind::System => {
                f.head(self.mark, self.name);
                f.pair("Distribution", "Linexin 2026.10");
                f.pair("Kernel", "7.2.8-2-cachyos");
                f.pair("Processor", "AMD Ryzen 5 7600 6-Core Processor");
                f.pair("Graphics", "AMD Radeon RX 9060 XT (RADV GFX1200)");
                f.pair("Memory", "31.2 GiB");
                f.gap();
                f.note("Every value is read from the running system when the page opens.");
                if f.button("Copy to clipboard") {
                    self.pressed = Some("copy".into());
                }
                if f.button("Refresh") {
                    self.pressed = Some("refresh".into());
                }
            }
            Kind::Settings => {
                f.head(self.mark, self.name);
                for index in 0..30 {
                    let label = format!("Setting {}", index + 1);
                    let pressed = if index % 3 == 1 {
                        f.row_value(&label, if index % 2 == 0 { "On" } else { "Off" })
                    } else {
                        f.row(&label)
                    };
                    if pressed {
                        self.pressed = Some(label);
                    }
                }
                f.note("Thirty rows: more than a window holds, so the page follows the light.");
            }
            Kind::Reading => {
                f.head(self.mark, self.name);
                f.text(
                    "A page in this language is read from the top, and the light lands on \
                     the first thing it can be on. Up past the first control lets go of the \
                     light to read what is above it; Down brings it back.",
                );
                f.rule();
                for name in [
                    "Connect automatically",
                    "Share with other users",
                    "Metered connection",
                ] {
                    if f.item(name) {
                        self.pressed = Some(name.into());
                    }
                }
                f.gap();
                if f.button("Apply") {
                    self.pressed = Some("apply".into());
                }
                f.note(
                    "Changes are applied when Apply is pressed. Nothing here is saved until \
                     then, and leaving the page keeps what it was.",
                );
            }
            Kind::About => {
                f.head(self.mark, "Linexin Center");
                f.text("The control centre of Linexin, here in LineXinBar's own language.");
                f.note("Licensed under the GPL, version 3 or later.");
            }
        }
        if let Some(pressed) = &self.pressed {
            f.note(&format!("Last pressed: {pressed}"));
        }
        f.finish();
        if menu {
            page.menu(Some(self.name), &["Copy", "Reset to default", "Close"]);
        }
        page_result(input.leave.is_some())
    }
}
