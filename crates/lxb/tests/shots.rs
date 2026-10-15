//! Pictures and checks against the installed libraries. Every test skips
//! itself, saying so, where `liblxb_app.so` cannot be opened. The pictures go
//! to `lxb-shots/` in cargo's temporary folder for tests (under the target
//! directory, wherever it is) to be looked at.

#[path = "../examples/common/mod.rs"]
mod common;

use common::{Demo, Kind, Raise};
use linexin_lxb::toolkit::{self, List};
use linexin_lxb::{Action, App, Beside, Metric, Overlay, Role, Slide, Sound, Surface, Text, load};
use std::path::PathBuf;
use std::sync::Mutex;

/// One GPU picture at a time.
static GPU: Mutex<()> = Mutex::new(());

fn library() -> bool {
    match load() {
        Ok(_) => true,
        Err(why) => {
            eprintln!("skipped: {why}");
            false
        }
    }
}

fn shots() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("lxb-shots");
    std::fs::create_dir_all(&dir).expect("shots directory");
    dir
}

fn shoot(name: &str, width: u32, height: u32, demo: &mut Demo) -> PathBuf {
    let _gpu = GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = shots().join(name);
    let mut app = App::new("com.linexin.LxbShots", "shots")
        .expect("app")
        .plain()
        .driven();
    app.shot(&path, width, height, 10.0, |page| demo.frame(page))
        .unwrap_or_else(|why| panic!("{name}: {why}"));
    let written = std::fs::metadata(&path).expect("written").len();
    assert!(written > 10_000, "{name} is only {written} bytes");
    path
}

fn demo_on(kind: Kind) -> (Demo, usize) {
    let demo = Demo::new();
    let index = demo
        .pages
        .iter()
        .position(|page| page.kind == kind)
        .expect("page");
    (demo, index)
}

#[test]
fn enumerations_match_the_librarys_own_lists() {
    if !library() {
        return;
    }
    fn check<T: Copy>(list: List, all: &[T], key: impl Fn(T) -> &'static str) {
        let Some(names) = toolkit::names(list) else {
            eprintln!("skipped {list:?}: no liblxb_toolkit");
            return;
        };
        let ours: Vec<&str> = all.iter().map(|value| key(*value)).collect();
        assert_eq!(names, ours, "{list:?}");
    }
    check(List::Action, Action::ALL, Action::key);
    check(List::Role, Role::ALL, Role::key);
    check(List::Metric, Metric::ALL, Metric::key);
    check(List::Text, Text::ALL, Text::key);
    check(List::Surface, Surface::ALL, Surface::key);
    check(List::Overlay, Overlay::ALL, Overlay::key);
    check(List::Sound, Sound::ALL, Sound::key);
    if toolkit::names(List::Glyph).is_some() {
        for mark in ["setting-info", "setting-display", "setting-network", "logo"] {
            assert!(toolkit::has_glyph(mark), "{mark}");
        }
        assert!(!toolkit::has_glyph("no-such-mark"));
    }
}

#[test]
fn the_loader_answers_what_it_found() {
    if !library() {
        return;
    }
    let lxb = load().expect("loaded");
    assert!(std::ptr::eq(lxb, load().expect("again")), "one instance");
    assert!(lxb.has("lxb_app_new"));
    assert!(lxb.has("lxb_draw_soft_edges"));
    assert!(!lxb.has("lxb_no_such_function"));
    eprintln!(
        "toolkit {:?}; missing from the installed libraries: {:?}",
        toolkit::version(),
        lxb.missing()
    );
}

#[test]
fn the_layout_answers_match_the_transcription() {
    if !library() {
        return;
    }
    for (window, margin, column, gap, least) in [
        (1920.0, 24.0, 300.0, 18.0, 600.0),
        (620.0, 16.0, 380.0, 12.0, 400.0),
        (700.0, 13.0, 407.0, 13.0, 509.0),
    ] {
        let laid = Beside::new(window, margin, column, gap, least);
        let page_x = margin + column + gap;
        assert_eq!(laid.page_x, page_x);
        assert!(laid.page_w > 0.0 && laid.reach >= 0.0);
    }
    let mut slide = Slide::default();
    assert_eq!(
        slide.follow(300.0, 0.016),
        300.0,
        "the first frame does not slide"
    );
    assert!(!slide.moving());
    let next = slide.follow(0.0, 0.016);
    assert!(next < 300.0 && next > 0.0);
    assert!(slide.moving());
    slide.place_again();
    assert_eq!(slide.follow(0.0, 0.016), 0.0);
}

/// The column has the light; the chosen page is a preview beside it.
#[test]
fn column_with_a_preview() {
    if !library() {
        return;
    }
    let (mut demo, index) = demo_on(Kind::System);
    demo.column.select(index);
    shoot("column-lit.png", 1280, 800, &mut demo);
    assert!(!demo.page_lit);
}

/// The light is on row 14 of a page longer than the window: the page has
/// scrolled to it and its ends are softened.
#[test]
fn scrolled_page_with_the_light_on_a_row() {
    if !library() {
        return;
    }
    let (mut demo, index) = demo_on(Kind::Settings);
    demo.column.select(index);
    demo.page_lit = true;
    demo.pages[index].flow.focus(13);
    shoot("page-scrolled.png", 1280, 800, &mut demo);
    let flow = &demo.pages[index].flow;
    assert_eq!(flow.focused(), Some(13));
    assert_eq!(flow.count(), 30);
    assert!(flow.scroll() > 0.0, "scrolled to the light");
}

#[test]
fn a_menu_raised_over_the_lit_row() {
    if !library() {
        return;
    }
    let (mut demo, index) = demo_on(Kind::Settings);
    demo.column.select(index);
    demo.page_lit = true;
    demo.pages[index].flow.focus(3);
    demo.pages[index].raise = Some(Raise::Menu);
    shoot("page-menu.png", 1280, 800, &mut demo);
}

#[test]
fn a_dialog_over_the_page() {
    if !library() {
        return;
    }
    let (mut demo, index) = demo_on(Kind::System);
    demo.column.select(index);
    demo.page_lit = true;
    demo.pages[index].raise = Some(Raise::Dialog);
    shoot("page-dialog.png", 1280, 800, &mut demo);
}

/// A window standing on its side: with the column lit the page peeks in at
/// the right; with the page lit the view has slid and a strip of the column
/// stays at the left.
#[test]
fn a_narrow_window_slides() {
    if !library() {
        return;
    }
    let (mut demo, index) = demo_on(Kind::Reading);
    demo.column.select(index);
    shoot("narrow-column.png", 700, 1000, &mut demo);
    let (mut demo, index) = demo_on(Kind::Reading);
    demo.column.select(index);
    demo.page_lit = true;
    shoot("narrow-page.png", 700, 1000, &mut demo);
}

/// `-w`: the page alone fills the window and has the light.
#[test]
fn single_widget_mode_fills_the_window() {
    if !library() {
        return;
    }
    let (mut demo, index) = demo_on(Kind::System);
    demo.column.select(index);
    demo.single = true;
    shoot("single.png", 1280, 800, &mut demo);
}

/// The same page drawn by the library's own flow (`lxb_page_row` and the
/// rest, focused with `lxb_page_focus`) and by [`Flow`]: the pictures are
/// compared, so the flow's sizes cannot drift from the library's.
#[test]
fn the_flow_draws_what_the_librarys_flow_draws() {
    if !library() {
        return;
    }
    use linexin_lxb::{Flow, Page, Panes};
    fn content(page: &Page) -> [f32; 4] {
        let panes = Panes::whole(page);
        page.draw_pane(panes.window, Overlay::Dialog);
        panes.content
    }
    const TEXT: &str = "A sentence long enough to be wrapped onto a second line of the page, \
                        so that the paragraph's own breaking is compared too: the words are \
                        carried to the next line where the next one would not fit.";
    let library_flow = shots().join("compare-library.png");
    let our_flow = shots().join("compare-flow.png");
    let _gpu = GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    App::new("com.linexin.LxbShots", "shots")
        .unwrap()
        .plain()
        .driven()
        .shot(&library_flow, 1000, 700, 10.0, |page| {
            let rect = content(page);
            page.set_cursor(rect);
            page.focus(1);
            page.text(TEXT);
            page.row("First row");
            page.row_value("Second row", "its value");
            page.rule();
            page.item("An item");
            page.note(TEXT);
            page.button("A button");
            page.heading("A heading");
            page.gap();
            page.title("A title");
        })
        .unwrap();
    let mut flow = Flow::new();
    flow.focus(1);
    App::new("com.linexin.LxbShots", "shots")
        .unwrap()
        .plain()
        .driven()
        .shot(&our_flow, 1000, 700, 10.0, |page| {
            let rect = content(page);
            let mut f = flow.begin(page, rect, true);
            f.text(TEXT);
            f.row("First row");
            f.row_value("Second row", "its value");
            f.rule();
            f.item("An item");
            f.note(TEXT);
            f.button("A button");
            f.heading("A heading");
            f.gap();
            f.title("A title");
        })
        .unwrap();
    let differing = differing_pixels(&library_flow, &our_flow);
    eprintln!("pixels differing by more than 8/255: {differing}");
    assert!(differing < 50, "{differing} pixels differ");
}

/// Fifteen Downs on a long page: the light is on the sixteenth row and the
/// page has scrolled to it, the rows above it left out and the ends soft.
#[test]
fn walking_down_a_long_page_scrolls_it() {
    if !library() {
        return;
    }
    let (mut demo, index) = demo_on(Kind::Settings);
    demo.column.select(index);
    demo.page_lit = true;
    let mut frames = 0;
    let _gpu = GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = shots().join("page-walked.png");
    let flow = &mut demo.pages[index].flow;
    App::new("com.linexin.LxbShots", "shots")
        .unwrap()
        .plain()
        .driven()
        .shot(&path, 1280, 800, 10.0, |page| {
            let rect = linexin_lxb::Panes::whole(page).content;
            if frames == 1 {
                let input = flow.input(page, &[Action::Down; 15]);
                assert_eq!(input.leave, None);
                flow.snap();
            }
            frames += 1;
            let mut f = flow.begin(page, rect, true);
            // Pinned: it stays at the top while the rows scroll under it.
            f.head("setting-system", "A long page");
            for row in 0..30 {
                f.row(&format!("Row {}", row + 1));
            }
        })
        .unwrap();
    assert_eq!(flow.focused(), Some(15));
    assert!(flow.scroll() > 0.0);
    let mut leaving = linexin_lxb::Flow::new();
    let shot = App::new("com.linexin.LxbShots", "shots")
        .unwrap()
        .plain()
        .driven()
        .shot(shots().join("page-left.png"), 320, 200, 0.0, |page| {
            let rect = linexin_lxb::Panes::whole(page).content;
            let input = leaving.input(page, &[Action::Down, Action::Left, Action::Down]);
            assert_eq!(input.leave, Some(linexin_lxb::Leave::Left));
            let mut f = leaving.begin(page, rect, true);
            f.row("Only");
        });
    shot.unwrap();
}

/// A column longer than its window keeps its chosen entry in view, and its
/// ends are softened.
#[test]
fn a_long_column_scrolls_to_its_choice() {
    if !library() {
        return;
    }
    use linexin_lxb::{Column, ColumnContent, ColumnEntry, Split};
    let names: Vec<String> = (1..=24).map(|index| format!("Widget {index}")).collect();
    let entries: Vec<ColumnEntry> = names
        .iter()
        .map(|name| ColumnEntry {
            mark: "setting-system",
            label: name,
            ..ColumnEntry::default()
        })
        .collect();
    let mut column = Column::new();
    column.select(17);
    let mut split = Split::new();
    let _gpu = GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    App::new("com.linexin.LxbShots", "shots")
        .unwrap()
        .plain()
        .driven()
        .shot(shots().join("column-long.png"), 1000, 600, 10.0, |page| {
            let panes = split.layout(page, false);
            let content = ColumnContent {
                entries: &entries,
                ..ColumnContent::default()
            };
            page.draw_pane(panes.window, Overlay::Dialog);
            column.draw(page, panes.column, &content, true);
        })
        .unwrap();
    assert_eq!(column.selected(), 17);
}

/// How many pixels of two PNGs of one size differ noticeably.
fn differing_pixels(a: &std::path::Path, b: &std::path::Path) -> usize {
    fn decode(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
        let file = std::fs::read(path).expect("picture");
        let (header, pixels) = png_rgba(&file);
        (header.0, header.1, pixels)
    }
    let (wa, ha, pa) = decode(a);
    let (wb, hb, pb) = decode(b);
    assert_eq!((wa, ha), (wb, hb));
    pa.chunks(4)
        .zip(pb.chunks(4))
        .filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > 8))
        .count()
}

/// A minimal PNG reader for the 8-bit RGBA pictures `lxb_app_shot` writes.
fn png_rgba(file: &[u8]) -> ((u32, u32), Vec<u8>) {
    let be = |bytes: &[u8]| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let mut at = 8;
    let (mut width, mut height) = (0, 0);
    let mut data = Vec::new();
    while at + 8 <= file.len() {
        let length = be(&file[at..]) as usize;
        let kind = &file[at + 4..at + 8];
        let body = &file[at + 8..at + 8 + length];
        match kind {
            b"IHDR" => {
                width = be(body);
                height = be(&body[4..]);
                assert_eq!((body[8], body[9]), (8, 6), "8-bit RGBA");
            }
            b"IDAT" => data.extend_from_slice(body),
            _ => {}
        }
        at += 12 + length;
    }
    let raw = inflate(&data);
    let stride = width as usize * 4;
    let mut pixels = vec![0u8; stride * height as usize];
    let mut previous = vec![0u8; stride];
    for row in 0..height as usize {
        let line = &raw[row * (stride + 1)..(row + 1) * (stride + 1)];
        let (filter, line) = (line[0], &line[1..]);
        let mut current = vec![0u8; stride];
        for index in 0..stride {
            let left = if index >= 4 { current[index - 4] } else { 0 };
            let up = previous[index];
            let corner = if index >= 4 { previous[index - 4] } else { 0 };
            let predicted = match filter {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                4 => {
                    let p = i16::from(left) + i16::from(up) - i16::from(corner);
                    let (pa, pb, pc) = (
                        (p - i16::from(left)).abs(),
                        (p - i16::from(up)).abs(),
                        (p - i16::from(corner)).abs(),
                    );
                    if pa <= pb && pa <= pc {
                        left
                    } else if pb <= pc {
                        up
                    } else {
                        corner
                    }
                }
                other => panic!("filter {other}"),
            };
            current[index] = line[index].wrapping_add(predicted);
        }
        pixels[row * stride..(row + 1) * stride].copy_from_slice(&current);
        previous = current;
    }
    ((width, height), pixels)
}

/// zlib inflate (stored, fixed and dynamic Huffman blocks).
fn inflate(data: &[u8]) -> Vec<u8> {
    struct Bits<'a> {
        data: &'a [u8],
        at: usize,
        bit: u32,
    }
    impl Bits<'_> {
        fn get(&mut self, count: u32) -> u32 {
            let mut value = 0;
            for index in 0..count {
                let byte = self.data[self.at];
                value |= u32::from((byte >> self.bit) & 1) << index;
                self.bit += 1;
                if self.bit == 8 {
                    self.bit = 0;
                    self.at += 1;
                }
            }
            value
        }
    }
    fn table(lengths: &[u8]) -> Vec<(u16, u8, u16)> {
        // (code, length, symbol), canonical Huffman.
        let mut codes = Vec::new();
        let mut code = 0u16;
        for length in 1..=15u8 {
            for (symbol, l) in lengths.iter().enumerate() {
                if *l == length {
                    codes.push((code, length, symbol as u16));
                    code += 1;
                }
            }
            code <<= 1;
        }
        codes
    }
    fn decode(bits: &mut Bits, codes: &[(u16, u8, u16)]) -> u16 {
        let (mut code, mut length) = (0u16, 0u8);
        loop {
            code = (code << 1) | bits.get(1) as u16;
            length += 1;
            if let Some(found) = codes.iter().find(|(c, l, _)| *l == length && *c == code) {
                return found.2;
            }
        }
    }
    const BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const EXTRA: [u32; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DBASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DEXTRA: [u32; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];
    let mut bits = Bits {
        data: &data[2..],
        at: 0,
        bit: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = bits.get(1);
        match bits.get(2) {
            0 => {
                if bits.bit != 0 {
                    bits.bit = 0;
                    bits.at += 1;
                }
                let length =
                    u16::from_le_bytes([bits.data[bits.at], bits.data[bits.at + 1]]) as usize;
                bits.at += 4;
                out.extend_from_slice(&bits.data[bits.at..bits.at + length]);
                bits.at += length;
            }
            kind => {
                let (literals, distances) = if kind == 1 {
                    let mut lengths = [8u8; 288];
                    lengths[144..256].fill(9);
                    lengths[256..280].fill(7);
                    (table(&lengths), table(&[5u8; 30]))
                } else {
                    let hlit = bits.get(5) as usize + 257;
                    let hdist = bits.get(5) as usize + 1;
                    let hclen = bits.get(4) as usize + 4;
                    const ORDER: [usize; 19] = [
                        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                    ];
                    let mut code_lengths = [0u8; 19];
                    for slot in ORDER.iter().take(hclen) {
                        code_lengths[*slot] = bits.get(3) as u8;
                    }
                    let codes = table(&code_lengths);
                    let mut lengths = Vec::new();
                    while lengths.len() < hlit + hdist {
                        match decode(&mut bits, &codes) {
                            symbol @ 0..=15 => lengths.push(symbol as u8),
                            16 => {
                                let previous = *lengths.last().unwrap();
                                for _ in 0..3 + bits.get(2) {
                                    lengths.push(previous);
                                }
                            }
                            17 => lengths.extend(std::iter::repeat_n(0, 3 + bits.get(3) as usize)),
                            _ => lengths.extend(std::iter::repeat_n(0, 11 + bits.get(7) as usize)),
                        }
                    }
                    (table(&lengths[..hlit]), table(&lengths[hlit..]))
                };
                loop {
                    let symbol = decode(&mut bits, &literals);
                    match symbol {
                        0..=255 => out.push(symbol as u8),
                        256 => break,
                        _ => {
                            let index = (symbol - 257) as usize;
                            let length = BASE[index] as usize + bits.get(EXTRA[index]) as usize;
                            let code = decode(&mut bits, &distances) as usize;
                            let distance = DBASE[code] as usize + bits.get(DEXTRA[code]) as usize;
                            let start = out.len() - distance;
                            for offset in 0..length {
                                out.push(out[start + offset]);
                            }
                        }
                    }
                }
            }
        }
        if last == 1 {
            break;
        }
    }
    out
}
