//! A small LineXinBar-style host: a column of pages and the chosen page
//! beside it, driven by keyboard, controller and pointer.
//!
//! ```text
//! cargo run -p linexin-lxb --example demo                  # the window
//! cargo run -p linexin-lxb --example demo -- --shot a.png  # one frame, no display
//! cargo run -p linexin-lxb --example demo -- --shot a.png 700 1000 page
//! ```
//! SIGINT or SIGTERM closes the window at the next frame.

mod common;

use common::Demo;
use linexin_lxb::App;
use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn stop(_signal: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let app = match App::new("com.linexin.LxbDemo", "Linexin Center (demo)") {
        Ok(app) => app,
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1);
        }
    };
    let mut app = app.plain().driven();
    let mut demo = Demo::new();

    let outcome = if arguments.first().map(String::as_str) == Some("--shot") {
        let path = arguments
            .get(1)
            .cloned()
            .unwrap_or_else(|| "demo.png".into());
        let width = arguments
            .get(2)
            .and_then(|value| value.parse().ok())
            .unwrap_or(1280);
        let height = arguments
            .get(3)
            .and_then(|value| value.parse().ok())
            .unwrap_or(800);
        demo.page_lit = arguments.get(4).map(String::as_str) == Some("page");
        app.shot(path, width, height, 10.0, |page| demo.frame(page))
    } else {
        // SAFETY: the handler only stores into an atomic.
        unsafe {
            libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
            libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
        }
        app.run(|page| {
            if STOP.load(Ordering::SeqCst) {
                eprintln!("demo: asked to stop, closing the window");
                page.quit();
                return;
            }
            // Low-end mode draws once a second while nothing moves; look
            // for the signal a little more often than that.
            page.redraw_within(0.25);
            demo.frame(page);
        })
    };
    if let Err(why) = outcome {
        eprintln!("{why}");
        std::process::exit(1);
    }
}
