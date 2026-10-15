//! What stands before a name: a widget's or an application's own icon,
//! drawn as the picture it is, with one of the shell's marks standing in for
//! it where the toolkit draws no pictures or the file cannot be read.
//!
//! DistriBumpy draws an application as its own icon and a place of its own
//! as a mark; a widget of the Center is an application in that sense.

use crate::page::Page;
use crate::types::Fit;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// How long a picture may take to be read before its mark stands in for it.
/// Until then nothing is drawn in its place: a mark that turned into a
/// picture a moment later would be one thing turning into another.
const GRACE: f32 = 1.5;

/// The sizes an icon theme's `apps` folders are looked through, largest
/// first (a picture is drawn larger than a list's mark in a page's heading).
const THEME_SIZES: &[&str] = &[
    "scalable", "512x512", "256x256", "192x192", "128x128", "96x96", "64x64", "48x48",
];

/// A mark, and the picture to draw instead of it where one can be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Emblem<'a> {
    /// One of the shell's marks (`lxb_glyph_name`), or empty.
    pub mark: &'a str,
    /// An image file (see [`icon_file`]).
    pub picture: Option<&'a Path>,
}

impl<'a> Emblem<'a> {
    /// One of the shell's marks alone.
    pub const fn mark(mark: &'a str) -> Self {
        Self {
            mark,
            picture: None,
        }
    }

    /// A picture, with `mark` standing in for it where it cannot be drawn.
    pub const fn with_picture(picture: Option<&'a Path>, mark: &'a str) -> Self {
        Self { mark, picture }
    }

    /// Whether there is nothing to draw.
    pub fn is_empty(&self) -> bool {
        self.mark.is_empty() && self.picture.is_none()
    }
}

impl<'a> From<&'a str> for Emblem<'a> {
    fn from(mark: &'a str) -> Self {
        Self::mark(mark)
    }
}

impl<'a> From<&'a String> for Emblem<'a> {
    fn from(mark: &'a String) -> Self {
        Self::mark(mark)
    }
}

/// When each picture was first asked for, and whether it was ever drawn.
static ASKED: Mutex<Option<HashMap<PathBuf, (f32, bool)>>> = Mutex::new(None);

/// Draw `emblem` in the square `rect`; the picture at `alpha`, `share` of
/// the square's size about its centre. A mark's drawing keeps air round it
/// inside its square and a picture fills its own to the edges: where the
/// square stands close to a capsule's rounded end, a picture is drawn smaller
/// for it to sit as a mark does.
pub(crate) fn draw(page: &Page, rect: [f32; 4], emblem: Emblem, alpha: f32, share: f32) {
    if let Some(path) = emblem.picture.filter(|_| page.draws_pictures()) {
        let size = rect[2].min(rect[3]) * share;
        let at = [
            rect[0] + (rect[2] - size) * 0.5,
            rect[1] + (rect[3] - size) * 0.5,
            size,
            size,
        ];
        let drawn = page.draw_picture(at, 0.0, path, Fit::Contain, alpha);
        let now = page.seconds();
        let mut asked = ASKED.lock().unwrap_or_else(PoisonError::into_inner);
        let known = asked
            .get_or_insert_with(HashMap::new)
            .entry(path.to_path_buf())
            .or_insert((now, false));
        if drawn {
            known.1 = true;
            return;
        }
        // Still being read (or read again after the toolkit let it go):
        // look again soon, even where frames wait for input.
        if known.1 || now - known.0 < GRACE {
            page.redraw_within(0.05);
            return;
        }
    }
    if !emblem.mark.is_empty() {
        page.draw_icon(rect, emblem.mark);
    }
}

/// The image file a widget's `icon` names, as the Center reads it: an
/// absolute path, looked for under `$XDG_DATA_DIRS` as well when it is under
/// `/usr/share` (a build staged somewhere else), or an icon theme's name
/// (`hicolor`'s applications, then `icons/` and `pixmaps/` themselves).
/// `None` when no file is found. Answers are kept for the process.
pub fn icon_file(icon: &str) -> Option<PathBuf> {
    static FOUND: Mutex<Option<HashMap<String, Option<PathBuf>>>> = Mutex::new(None);
    let mut found = FOUND.lock().unwrap_or_else(PoisonError::into_inner);
    found
        .get_or_insert_with(HashMap::new)
        .entry(icon.to_owned())
        .or_insert_with(|| look_for(icon, &data_dirs()))
        .clone()
}

/// `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`, with their defaults.
fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/share")));
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(
            dirs.split(':')
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from),
        )
        .collect()
}

fn look_for(icon: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    if icon.is_empty() {
        return None;
    }
    if icon.contains('/') {
        let path = Path::new(icon);
        if path.is_file() {
            return Some(path.to_path_buf());
        }
        let rest = icon.strip_prefix("/usr/share/")?;
        return dirs
            .iter()
            .map(|dir| dir.join(rest))
            .find(|path| path.is_file());
    }
    let named = |folder: PathBuf| {
        ["svg", "png"]
            .iter()
            .map(move |extension| folder.join(format!("{icon}.{extension}")))
    };
    dirs.iter()
        .flat_map(|dir| {
            let theme = dir.join("icons/hicolor");
            THEME_SIZES
                .iter()
                .flat_map(move |size| named(theme.join(size).join("apps")))
                .chain(named(dir.join("icons")))
                .chain(named(dir.join("pixmaps")))
        })
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn icons_are_found_where_the_center_finds_them() {
        let root = std::env::temp_dir().join(format!("lxb-emblem-{}", std::process::id()));
        let (staged, system) = (root.join("stage/share"), root.join("usr/share"));
        touch(&staged.join("icons/lxb-emblem-test.svg"));
        touch(&system.join("icons/hicolor/48x48/apps/thing.png"));
        touch(&system.join("icons/hicolor/scalable/apps/thing.svg"));
        touch(&system.join("pixmaps/old.png"));
        let dirs = [staged.clone(), system.clone()];

        let absolute = staged.join("icons/lxb-emblem-test.svg");
        assert_eq!(
            look_for(absolute.to_str().unwrap(), &dirs),
            Some(absolute.clone())
        );
        assert_eq!(
            look_for("/usr/share/icons/lxb-emblem-test.svg", &dirs),
            Some(absolute),
            "a staged build's icon is found under XDG_DATA_DIRS"
        );
        assert_eq!(
            look_for("thing", &dirs),
            Some(system.join("icons/hicolor/scalable/apps/thing.svg")),
            "the largest of a theme's sizes"
        );
        assert_eq!(look_for("old", &dirs), Some(system.join("pixmaps/old.png")));
        assert_eq!(look_for("missing", &dirs), None);
        assert_eq!(look_for("/opt/nowhere/x.png", &dirs), None);
        assert_eq!(look_for("", &dirs), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_mark_alone_is_an_emblem() {
        let emblem: Emblem = "logo".into();
        assert_eq!(emblem, Emblem::mark("logo"));
        assert!(!emblem.is_empty());
        assert!(Emblem::default().is_empty());
        assert!(!Emblem::with_picture(Some(Path::new("/x.png")), "").is_empty());
    }
}
