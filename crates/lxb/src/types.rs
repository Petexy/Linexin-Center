//! The enumerations that cross into the libraries, as typed values.
//!
//! Every value is the index `lxb_toolkit.h` gives it (the same index the
//! library's own `lxb_*_name` list uses); [`Action::key`] and friends are the
//! names that list answers, which the tests check against the library.

use std::ffi::c_ulong;

macro_rules! enumeration {
    (
        $(#[$meta:meta])*
        $name:ident {
            $( $(#[$variant_meta:meta])* $variant:ident = $value:literal => $key:literal, )*
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $(#[$variant_meta])* $variant = $value, )*
        }

        impl $name {
            /// Every value, in index order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            /// The index the C API takes.
            pub const fn raw(self) -> u32 {
                self as u32
            }

            /// The value at a C index, if this crate knows it.
            pub const fn from_raw(raw: u32) -> Option<Self> {
                match raw {
                    $( $value => Some(Self::$variant), )*
                    _ => None,
                }
            }

            /// The name the library's own list gives this value.
            pub const fn key(self) -> &'static str {
                match self {
                    $( Self::$variant => $key, )*
                }
            }

            #[allow(dead_code)]
            pub(crate) const fn ulong(self) -> c_ulong {
                self as c_ulong
            }
        }
    };
}

enumeration! {
    /// What the user asked for, whichever control said it (`lxb_action`).
    Action {
        Left = 0 => "left",
        Right = 1 => "right",
        Up = 2 => "up",
        Down = 3 => "down",
        Accept = 4 => "accept",
        Back = 5 => "back",
        Menu = 6 => "menu",
        Submit = 7 => "submit",
        Previous = 8 => "previous",
        Next = 9 => "next",
    }
}

impl Action {
    /// The actions of an `LxbFrameV1` (or any list of C indices), dropping
    /// indices this crate does not know.
    pub fn from_raw_list(raw: &[std::ffi::c_int]) -> Vec<Self> {
        raw.iter()
            .filter_map(|value| u32::try_from(*value).ok().and_then(Self::from_raw))
            .collect()
    }
}

enumeration! {
    /// A sheet of the shell's glass (`lxb_overlay`).
    Overlay {
        ContextMenu = 0 => "context-menu",
        Dialog = 1 => "dialog",
    }
}

enumeration! {
    /// One of the three cuts of glass (`LXB_SURFACE_*`).
    Surface {
        Panel = 0 => "panel",
        Control = 1 => "control",
        Sidebar = 2 => "sidebar",
    }
}

enumeration! {
    /// A colour role (`LXB_ROLE_*`).
    Role {
        Accent = 0 => "accent",
        AccentSoft = 1 => "accent-soft",
        AccentDeep = 2 => "accent-deep",
        Glass = 3 => "glass",
        GlassRaised = 4 => "glass-raised",
        Rim = 5 => "rim",
        Text = 6 => "text",
        TextSoft = 7 => "text-soft",
        Danger = 8 => "danger",
        Glow = 9 => "glow",
        SkyTop = 10 => "sky-top",
        SkyBottom = 11 => "sky-bottom",
        SkyTopAlt = 12 => "sky-top-alt",
        SkyBottomAlt = 13 => "sky-bottom-alt",
    }
}

enumeration! {
    /// A size of type (`LXB_TEXT_*`).
    Text {
        Display = 0 => "display",
        Title = 1 => "title",
        Body = 2 => "body",
        Label = 3 => "label",
        Caption = 4 => "caption",
    }
}

enumeration! {
    /// A named length (`LXB_METRIC_*`), answered for the window's height.
    Metric {
        CardRadius = 0 => "card-radius",
        PanelRadius = 1 => "panel-radius",
        PanelInset = 2 => "panel-inset",
        RowHeight = 3 => "row-height",
        RowPadding = 4 => "row-padding",
        PanelPadding = 5 => "panel-padding",
        Tile = 6 => "tile",
        Gap = 7 => "gap",
        TileGlyph = 8 => "tile-glyph",
        TileRadius = 9 => "tile-radius",
        ItemSpacing = 10 => "item-spacing",
        ColumnSpacing = 11 => "column-spacing",
        ItemIcon = 12 => "item-icon",
        ItemIconFocused = 13 => "item-icon-focused",
        ColumnIcon = 14 => "column-icon",
        ColumnIconFocused = 15 => "column-icon-focused",
        MenuWidth = 16 => "menu-width",
        DialogWidth = 17 => "dialog-width",
        DialogDim = 18 => "dialog-dim",
        PowerWidth = 19 => "power-width",
        PowerDim = 20 => "power-dim",
    }
}

enumeration! {
    /// Where a label stands in its rectangle (`LXB_ALIGN_*`).
    Align {
        Left = 0 => "left",
        Centre = 1 => "centre",
        Right = 2 => "right",
    }
}

enumeration! {
    /// One of the language's recordings (`enum lxb_sound`); the key is the
    /// recording's file name.
    Sound {
        Move = 0 => "press",
        Press = 1 => "press-selected",
        GuideMove = 2 => "press-guide",
        GuidePress = 3 => "press-guide-selected",
        Back = 4 => "press-back",
        Key = 5 => "keyboard-click",
        Launch = 6 => "app-launch",
        GuideOpen = 7 => "guide-open",
        Shutter = 8 => "screenshot",
        Authenticate = 9 => "polkit",
        Notify = 10 => "notification",
        Trash = 11 => "trash",
        Error = 12 => "error",
        Music = 13 => "start-bg-music",
    }
}

enumeration! {
    /// What a file question asks for (`LXB_PICKER_*`).
    PickerSelection {
        File = 0 => "file",
        Image = 1 => "image",
        Scenery = 2 => "scenery",
        Folder = 3 => "folder",
    }
}

/// How to draw a control of your own (`lxb_press`): resting, lit, or part of
/// the way through a press (0 to 1).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Press {
    #[default]
    Resting,
    Focused,
    Pressed,
    Going(f32),
}

/// How far down a press is when it is simply "pressed" (`motion::PRESS_DOWN`).
const PRESS_DOWN: f32 = 0.3;

impl Press {
    pub fn raw(self) -> c_ulong {
        match self {
            Press::Resting => 0,
            Press::Focused => 1,
            Press::Pressed => 2,
            Press::Going(through) => 3 + (through.clamp(0.0, 1.0) * 100.0).round() as c_ulong,
        }
    }

    pub fn from_raw(raw: c_ulong) -> Self {
        match raw {
            0 => Press::Resting,
            1 => Press::Focused,
            2 => Press::Pressed,
            going => Press::Going(((going - 3) as f32 / 100.0).clamp(0.0, 1.0)),
        }
    }

    /// Whether the light is on the control.
    pub fn lit(self) -> bool {
        self != Press::Resting
    }

    /// How far through a press it is, if one is under way.
    pub fn through(self) -> Option<f32> {
        match self {
            Press::Resting | Press::Focused => None,
            Press::Pressed => Some(PRESS_DOWN),
            Press::Going(through) => Some(through),
        }
    }

    /// How strongly the light is drawn under a control in this state: it
    /// dims while the control is pressed into the page, as the flow's own
    /// light does.
    pub fn light_strength(self) -> f32 {
        match self.through() {
            Some(through) => 1.0 - through * 0.35,
            None => 1.0,
        }
    }
}

impl From<bool> for Press {
    fn from(lit: bool) -> Self {
        if lit { Press::Focused } else { Press::Resting }
    }
}

/// How a picture fills the rectangle it is drawn in (`lxb_draw_picture`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// Whole, as large as it fits.
    #[default]
    Contain = 0,
    /// Filling the rectangle, cut where it overflows.
    Cover = 1,
}

impl Fit {
    pub(crate) const fn ulong(self) -> c_ulong {
        self as c_ulong
    }
}

/// Keep the screen lit, the machine awake, or both (`lxb_page_keep_awake`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hold {
    pub screen: bool,
    pub sleep: bool,
}

impl Hold {
    pub const NOTHING: Hold = Hold {
        screen: false,
        sleep: false,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_the_header_indices() {
        for (index, action) in Action::ALL.iter().enumerate() {
            assert_eq!(action.raw() as usize, index);
            assert_eq!(Action::from_raw(index as u32), Some(*action));
        }
        assert_eq!(Action::from_raw(10), None);
        assert_eq!(Sound::Move.raw(), 0);
        assert_eq!(Sound::Error.raw(), 12);
        assert_eq!(Metric::PowerDim.raw(), 20);
        assert_eq!(Role::SkyBottomAlt.raw(), 13);
        assert_eq!(PickerSelection::Folder.raw(), 3);
        assert_eq!(Metric::ALL.len(), 21);
        assert_eq!(Role::ALL.len(), 14);
        assert_eq!(Sound::ALL.len(), 14);
    }

    #[test]
    fn a_frames_actions_skip_what_is_unknown() {
        assert_eq!(
            Action::from_raw_list(&[3, -1, 4, 42, 5]),
            vec![Action::Down, Action::Accept, Action::Back]
        );
    }

    #[test]
    fn press_crosses_both_ways() {
        for press in [
            Press::Resting,
            Press::Focused,
            Press::Pressed,
            Press::Going(0.0),
            Press::Going(0.42),
            Press::Going(1.0),
        ] {
            assert_eq!(Press::from_raw(press.raw()), press);
        }
        assert_eq!(Press::Going(0.5).raw(), 53);
        assert!(!Press::Resting.lit());
        assert!(Press::Going(0.2).lit());
        assert_eq!(Press::Focused.light_strength(), 1.0);
        assert!((Press::Going(1.0).light_strength() - 0.65).abs() < 1e-6);
    }
}
