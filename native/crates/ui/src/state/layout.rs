//! The window's layout as pure state (`web/app.js` `SIDEBAR` and the drags
//! at lines 4165-4218; inventory `app.md` 3.1, 3.2, `style-viewer-routes.md`
//! 1.4): the sidebar's width and whether it shows, the chat column's width
//! beside the world, and where the window was.
//!
//! The grid is `sidebar, chat, 5 px divider, world`. The chat column's
//! default is `minmax(360px, 42%)` of the layout's width; once dragged it
//! is the dragged width. At 980 px and below the window is narrow
//! (`style.css` S:1478-1528): one column of the sidebar as a bar, the
//! world, a 1 px line and the chat ([`narrow_rows`]); the sidebar's list is
//! a drawer over the page ([`Drawer`]). [`Layout`] keeps the wide window's
//! values untouched by it, as the page does.

/// The sidebar's default width.
pub const SIDEBAR_WIDTH: f32 = 236.;
/// The sidebar's narrowest.
pub const SIDEBAR_MIN: f32 = 200.;
/// The sidebar's widest.
pub const SIDEBAR_MAX: f32 = 420.;
/// Dragged under this, the sidebar collapses.
pub const SIDEBAR_COLLAPSE_BELOW: f32 = 140.;
/// Pointer movement under this is a click, not a drag.
pub const DRAG_SLOP: f32 = 3.;
/// The divider's width.
pub const DIVIDER: f32 = 5.;
/// The chat column's narrowest after a drag.
pub const CHAT_MIN: f32 = 320.;
/// The room the world keeps when the chat column is dragged.
pub const WORLD_MIN: f32 = 360.;
/// The chat column's default: at least this.
pub const CHAT_DEFAULT_MIN: f32 = 360.;
/// The chat column's default: this share of the layout's width.
pub const CHAT_DEFAULT_SHARE: f32 = 0.42;
/// The narrow layout's breakpoint (`NARROW`, `(max-width: 980px)`).
pub const NARROW_BELOW: f32 = 980.;

/// The sidebar and the split, as remembered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The sidebar shows (`mcb-sidebar`).
    pub sidebar_open: bool,
    /// Its width (`mcb-sidebar-width`), kept while it is collapsed.
    pub sidebar_width: f32,
    /// The chat column's dragged width (`mcb-chat-width`); none is the
    /// default `minmax(360px, 42%)`.
    pub chat_width: Option<f32>,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            sidebar_open: true,
            sidebar_width: SIDEBAR_WIDTH,
            chat_width: None,
        }
    }
}

impl Layout {
    /// The sidebar's column width as drawn (0 when it is closed).
    pub fn sidebar_column(&self) -> f32 {
        if self.sidebar_open {
            self.sidebar_width
        } else {
            0.
        }
    }

    /// The chat column's width in a layout `width` wide: the dragged width,
    /// or `minmax(360px, 42%)`. The world takes what is left, at least 0.
    pub fn chat_column(&self, width: f32) -> f32 {
        match self.chat_width {
            Some(w) => w,
            None => (width * CHAT_DEFAULT_SHARE).max(CHAT_DEFAULT_MIN),
        }
    }

    /// Mod+B in a wide window: shows or hides the sidebar, keeping its width.
    pub fn toggle_sidebar(&mut self) {
        self.sidebar_open = !self.sidebar_open;
    }
}

/// A drag of the sidebar's edge (or of the reveal strip of a closed one).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SidebarDrag {
    /// The pointer's x at the press.
    pub from: f32,
    /// The layout's left edge.
    pub left: f32,
    /// The width when the drag began.
    pub start_width: f32,
    /// Whether the sidebar showed when it began.
    pub start_open: bool,
    /// Moved at least [`DRAG_SLOP`].
    pub moved: bool,
}

impl SidebarDrag {
    /// A press on the edge at `x`.
    pub fn start(layout: &Layout, x: f32, left: f32) -> Self {
        SidebarDrag {
            from: x,
            left,
            start_width: layout.sidebar_width,
            start_open: layout.sidebar_open,
            moved: false,
        }
    }

    /// The pointer at `x`: under 140 px the sidebar collapses (keeping its
    /// width), pulled back out it opens at the pointer, clamped to 200 to
    /// 420 and rounded.
    pub fn move_to(&mut self, layout: &mut Layout, x: f32) {
        if !self.moved && (x - self.from).abs() < DRAG_SLOP {
            return;
        }
        self.moved = true;
        let width = x - self.left;
        if width < SIDEBAR_COLLAPSE_BELOW {
            layout.sidebar_open = false;
        } else {
            layout.sidebar_open = true;
            layout.sidebar_width = width.clamp(SIDEBAR_MIN, SIDEBAR_MAX).round();
        }
    }

    /// The release: a click without movement on a collapsed sidebar opens
    /// it; a drag that ended collapsed restores the width it began with.
    pub fn end(self, layout: &mut Layout) {
        if !self.moved {
            if !self.start_open {
                layout.sidebar_open = true;
            }
            return;
        }
        if !layout.sidebar_open {
            layout.sidebar_width = self.start_width;
        }
    }
}

/// The chat column's width for the divider dragged to `x`: from the chat's
/// left edge, at least 320 and at most `max(room - 360, 320)`, where room
/// is the layout's right edge less the chat's left edge less the divider;
/// rounded.
pub fn chat_drag(x: f32, chat_left: f32, layout_right: f32) -> f32 {
    let room = layout_right - chat_left - DIVIDER;
    (x - chat_left)
        .clamp(CHAT_MIN, (room - WORLD_MIN).max(CHAT_MIN))
        .round()
}

/// A rectangle in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left.
    pub x: f32,
    /// Top.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// Where the window was: its display and its bounds on the desktop.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// The display's identity (its UUID), as GPUI reports it.
    pub display: String,
    /// The window's bounds.
    pub bounds: Rect,
}

/// Where a remembered window opens: on its display when that display is
/// still there, else on the primary; moved (and shrunk if it must) so it
/// lies wholly on that display's visible area. `displays` is each
/// display's identity and visible bounds, the primary first.
pub fn restore(saved: &Placement, displays: &[(String, Rect)]) -> (String, Rect) {
    let (id, area) = displays
        .iter()
        .find(|(id, _)| *id == saved.display)
        .or(displays.first())
        .expect("at least one display");
    let width = saved.bounds.width.min(area.width);
    let height = saved.bounds.height.min(area.height);
    let x = saved.bounds.x.clamp(area.x, area.x + area.width - width);
    let y = saved.bounds.y.clamp(area.y, area.y + area.height - height);
    (
        id.clone(),
        Rect {
            x,
            y,
            width,
            height,
        },
    )
}

/// Where the first window opens when no placement is stored: `size`, shrunk
/// to fit if it must, centred in the display's visible area (`area`: the
/// display less its menu bar, Dock or taskbar, as GPUI reports it), as the
/// platform centres a new window. The bounds are the window's outer frame
/// on every platform (on macOS the vendored backend makes them so, patch 35
/// of `native/vendor/README.md`), so the whole window lies inside `area`.
pub fn first(area: Rect, size: (f32, f32)) -> Rect {
    let width = size.0.min(area.width);
    let height = size.1.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2.,
        y: area.y + (area.height - height) / 2.,
        width,
        height,
    }
}

// narrow agent: the narrow window (`NARROW`, `components.md` G.1,
// `style-viewer-routes.md` 1.20).

/// The bar's row (`grid-template-rows: 52px ...`).
pub const BAR: f32 = 52.;
/// The world's share of the window's height (`38dvh`, `min-height: 38vh`).
pub const NARROW_WORLD_SHARE: f32 = 0.38;
/// The line between the world and the chat.
pub const NARROW_DIVIDER: f32 = 1.;
/// The drawer's widest (`width: min(340px, 88vw)`).
pub const DRAWER_MAX: f32 = 340.;
/// The drawer's share of the window's width.
pub const DRAWER_SHARE: f32 = 0.88;
/// At this width and below the prompt palette is one column without its
/// preview, 300 px high (`(max-width: 720px)`, S:2046-2049).
pub const PALETTE_ONE_COLUMN_AT: f32 = 720.;
/// The smallest window the app allows (`WindowOptions::window_min_size`):
/// the page's 560 px rules never apply.
pub const WINDOW_MIN: (f32, f32) = (640., 420.);

/// A window `width` wide is narrow (`(max-width: 980px)`: 980 is narrow).
pub fn narrow(width: f32) -> bool {
    width <= NARROW_BELOW
}

/// The narrow window's rows, top to bottom, in a window `height` high.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NarrowRows {
    /// The sidebar as a bar.
    pub bar: f32,
    /// The world (0 for the prompt library, which has none).
    pub world: f32,
    /// The line under the world (0 for the prompt library).
    pub divider: f32,
    /// The chat column, what is left (`minmax(0, 1fr)`).
    pub chat: f32,
}

/// The rows `52px 38dvh 1px minmax(0, 1fr)`; the prompt library's are
/// `52px minmax(0, 1fr)` (S:1973-1976).
pub fn narrow_rows(height: f32, prompts: bool) -> NarrowRows {
    let (world, divider) = if prompts {
        (0., 0.)
    } else {
        (height * NARROW_WORLD_SHARE, NARROW_DIVIDER)
    };
    NarrowRows {
        bar: BAR,
        world,
        divider,
        chat: (height - BAR - world - divider).max(0.),
    }
}

/// The drawer's width in a window `width` wide.
pub fn drawer_width(width: f32) -> f32 {
    DRAWER_MAX.min(width * DRAWER_SHARE)
}

/// The prompt palette shows its preview beside its list.
pub fn palette_preview(width: f32) -> bool {
    width > PALETTE_ONE_COLUMN_AT
}

/// The narrow window's list drawer: open or not, never kept; the sidebar's
/// own open and width are the wide window's (`drawerOpen`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drawer {
    /// Asked open. It shows only while the window is narrow, and stays
    /// asked across a widening, as the page's state does.
    pub open: bool,
}

impl Drawer {
    /// Whether it shows in a window `width` wide (`drawer`).
    pub fn shown(&self, width: f32) -> bool {
        narrow(width) && self.open
    }
}

/// The sidebar button and the sidebar bind (`toggleSidebar`): narrow, the
/// drawer opens or closes; wide, the sidebar hides or shows.
pub fn toggle_sidebar(layout: &mut Layout, drawer: &mut Drawer, width: f32) {
    if narrow(width) {
        drawer.open = !drawer.open;
    } else {
        layout.toggle_sidebar();
    }
}

/// The sidebar button's name (`sidebarLabel`): `Collapse sidebar` wide;
/// narrow `Close the list` while the drawer is open, else what the list
/// holds, `Benches and prompts` or `Chats`.
pub fn sidebar_toggle_label(width: f32, drawer_open: bool, bench: bool) -> &'static str {
    if !narrow(width) {
        "Collapse sidebar"
    } else if drawer_open {
        "Close the list"
    } else if bench {
        "Benches and prompts"
    } else {
        "Chats"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_window_fits_the_visible_area_centred() {
        // A 1440 by 900 screen less a 25-point menu bar: 1280 by 820 fits,
        // centred, and its bottom edge is on screen.
        let mac = Rect {
            x: 0.,
            y: 25.,
            width: 1440.,
            height: 875.,
        };
        let placed = first(mac, (1280., 820.));
        assert_eq!(
            placed,
            Rect {
                x: 80.,
                y: 52.5,
                width: 1280.,
                height: 820.
            }
        );
        assert!(placed.y + placed.height <= mac.y + mac.height);
        // A small display (1024 by 768 less a 40-pixel taskbar at the
        // bottom): shrunk to the visible area and placed on it.
        let small = Rect {
            x: 0.,
            y: 0.,
            width: 1024.,
            height: 728.,
        };
        assert_eq!(first(small, (1280., 820.)), small);
        // Narrow only: the width shrinks, the height is centred.
        let narrow = Rect {
            x: 1920.,
            y: 0.,
            width: 800.,
            height: 1200.,
        };
        assert_eq!(
            first(narrow, (1280., 820.)),
            Rect {
                x: 1920.,
                y: 190.,
                width: 800.,
                height: 820.
            }
        );
    }

    #[test]
    fn the_chat_column_defaults_to_minmax_360_42_percent() {
        let layout = Layout::default();
        assert_eq!(layout.chat_column(1280.), 1280. * 0.42);
        assert_eq!(layout.chat_column(700.), 360.);
        let dragged = Layout {
            chat_width: Some(500.),
            ..layout
        };
        assert_eq!(dragged.chat_column(1280.), 500.);
    }

    #[test]
    fn the_divider_keeps_320_for_the_chat_and_360_for_the_world() {
        // Chat from x 236, layout to x 1280: room 1039, at most 679.
        assert_eq!(chat_drag(600.4, 236., 1280.), 364.);
        assert_eq!(chat_drag(300., 236., 1280.), 320.);
        assert_eq!(chat_drag(1270., 236., 1280.), 679.);
        // A window too small for both keeps the chat at 320.
        assert_eq!(chat_drag(900., 236., 800.), 320.);
    }

    #[test]
    fn the_sidebar_drag_clamps_collapses_and_restores() {
        let mut layout = Layout::default();
        let mut drag = SidebarDrag::start(&layout, 236., 0.);
        drag.move_to(&mut layout, 237.);
        assert!(!drag.moved, "under 3 px is not a drag");
        drag.move_to(&mut layout, 500.7);
        assert_eq!(layout.sidebar_width, 420.);
        drag.move_to(&mut layout, 170.2);
        assert_eq!((layout.sidebar_open, layout.sidebar_width), (true, 200.));
        drag.move_to(&mut layout, 120.);
        assert!(!layout.sidebar_open, "under 140 it collapses");
        drag.end(&mut layout);
        assert_eq!(
            layout.sidebar_width, 236.,
            "collapsing keeps the width it began with"
        );
        // A click on the collapsed sidebar's strip opens it.
        let click = SidebarDrag::start(&layout, 2., 0.);
        click.end(&mut layout);
        assert!(layout.sidebar_open);
        // Pulled back out, it opens at the pointer.
        layout.sidebar_open = false;
        let mut reveal = SidebarDrag::start(&layout, 2., 0.);
        reveal.move_to(&mut layout, 300.);
        reveal.end(&mut layout);
        assert_eq!((layout.sidebar_open, layout.sidebar_width), (true, 300.));
    }

    #[test]
    fn a_window_reopens_on_its_display_clamped_onto_it() {
        let primary = (
            "p".to_string(),
            Rect {
                x: 0.,
                y: 0.,
                width: 1440.,
                height: 900.,
            },
        );
        let second = (
            "s".to_string(),
            Rect {
                x: 1440.,
                y: 0.,
                width: 1920.,
                height: 1080.,
            },
        );
        let on_second = Placement {
            display: "s".into(),
            bounds: Rect {
                x: 1600.,
                y: 100.,
                width: 1280.,
                height: 820.,
            },
        };
        assert_eq!(
            restore(&on_second, &[primary.clone(), second.clone()]),
            ("s".to_string(), on_second.bounds)
        );
        // Its display is gone: the primary, moved onto it.
        let (id, bounds) = restore(&on_second, std::slice::from_ref(&primary));
        assert_eq!(id, "p");
        assert_eq!(
            bounds,
            Rect {
                x: 160.,
                y: 80.,
                width: 1280.,
                height: 820.
            }
        );
        // Larger than the display: shrunk to it.
        let huge = Placement {
            display: "p".into(),
            bounds: Rect {
                x: -50.,
                y: -50.,
                width: 3000.,
                height: 2000.,
            },
        };
        assert_eq!(
            restore(&huge, &[primary]).1,
            Rect {
                x: 0.,
                y: 0.,
                width: 1440.,
                height: 900.
            }
        );
    }

    // narrow agent.
    #[test]
    fn the_window_is_narrow_at_980_and_below() {
        assert!(narrow(980.));
        assert!(narrow(979.));
        assert!(narrow(WINDOW_MIN.0));
        assert!(!narrow(981.));
        assert!(!narrow(1280.));
    }

    #[test]
    fn the_narrow_rows_are_a_bar_the_world_a_line_and_the_chat() {
        assert_eq!(
            narrow_rows(820., false),
            NarrowRows {
                bar: 52.,
                world: 820. * 0.38,
                divider: 1.,
                chat: 820. - 52. - 820. * 0.38 - 1.
            }
        );
        // The smallest window keeps room for the chat.
        let small = narrow_rows(WINDOW_MIN.1, false);
        assert!(small.chat > 200., "{small:?}");
        // The prompt library has no world.
        assert_eq!(
            narrow_rows(820., true),
            NarrowRows {
                bar: 52.,
                world: 0.,
                divider: 0.,
                chat: 768.
            }
        );
    }

    #[test]
    fn the_drawer_is_340_or_88_percent_and_shows_only_narrow() {
        assert_eq!(drawer_width(979.), 340.);
        assert_eq!(drawer_width(WINDOW_MIN.0), 340.);
        assert_eq!(drawer_width(300.), 264.);
        let mut layout = Layout::default();
        let mut drawer = Drawer::default();
        toggle_sidebar(&mut layout, &mut drawer, 800.);
        assert!(drawer.shown(800.));
        assert!(layout.sidebar_open, "the wide sidebar is untouched");
        // Widened, it does not show but stays asked; narrowed again, it is
        // back, as the page's state is.
        assert!(!drawer.shown(1200.));
        assert!(drawer.shown(979.));
        // Wide, the button hides the sidebar and leaves the drawer.
        toggle_sidebar(&mut layout, &mut drawer, 1200.);
        assert!(!layout.sidebar_open);
        assert!(drawer.open);
    }

    #[test]
    fn the_sidebar_button_is_named_for_what_it_does() {
        assert_eq!(sidebar_toggle_label(1280., false, true), "Collapse sidebar");
        assert_eq!(sidebar_toggle_label(1280., true, false), "Collapse sidebar");
        assert_eq!(sidebar_toggle_label(980., false, false), "Chats");
        assert_eq!(
            sidebar_toggle_label(800., false, true),
            "Benches and prompts"
        );
        assert_eq!(sidebar_toggle_label(640., true, true), "Close the list");
    }

    #[test]
    fn the_palette_drops_its_preview_at_720_and_below() {
        assert!(palette_preview(721.));
        assert!(!palette_preview(720.));
        assert!(!palette_preview(WINDOW_MIN.0));
    }
}
