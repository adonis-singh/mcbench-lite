//! The page's line icons (`web/app.js` `Icon`, inventory `components.md`
//! 1.1 and B.1) and vendor logos (`Logo`, `web/vendor-icon.mjs`, the files
//! of `web/icons/`), drawn by GPUI as vector pictures.
//!
//! An icon is a 24 by 24 drawing, stroke 1.75 with round caps and joins, in
//! the current text colour; [`icon`] draws one at the size the page gives it.
//! The paths are the page's, character for character (`grip`'s eight dots,
//! which the page builds in a loop, written out) (the page defines
//! `eye` twice and only the first is reachable; that is the one here).
//! GPUI draws an SVG as an alpha mask painted in the element's text colour,
//! which is what the page's `stroke="currentColor"` and its `.vlogo` mask
//! (`background: currentColor`) do.
//!
//! A logo ([`logo`]) is the page's file painted in the text colour, or in
//! its brand fill (`BRAND`, `web/app.js:540-549`): a solid brand colour is
//! the mask's colour; a gradient brand (Qwen, Gemini, Meta, Mistral) is
//! drawn as a picture whose gradient is masked by the logo, as the page's
//! CSS mask over a gradient background.
//!
//! Contract: an icon is decoration (the page's `aria-hidden`). It never
//! carries a control's only name: every control that shows an icon has its
//! accessible name set by the control ([`super::controls`]); the icon
//! element itself adds no node to the accessibility tree.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use gpui_kit::{
    AssetSource, Image, ImageFormat, IntoElement, ParentElement as _, Pixels, RenderOnce,
    SharedString, Styled as _, div, img, px, svg,
};

use super::colour;

/// Every icon of the page's `Icon`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    /// `send`.
    Send,
    /// `eye`.
    Eye,
    /// `eye-off`.
    EyeOff,
    /// `trophy`.
    Trophy,
    /// `play`.
    Play,
    /// `pause`.
    Pause,
    /// `step`.
    Step,
    /// `restart`.
    Restart,
    /// `stop`.
    Stop,
    /// `settings`.
    Settings,
    /// `copy`.
    Copy,
    /// `check`.
    Check,
    /// `compose`.
    Compose,
    /// `panel`.
    Panel,
    /// `archive`.
    Archive,
    /// `star`.
    Star,
    /// `sun`.
    Sun,
    /// `moon`.
    Moon,
    /// `search`.
    Search,
    /// `x`.
    X,
    /// `equal`.
    Equal,
    /// `trash`.
    Trash,
    /// `share`.
    Share,
    /// `build`.
    Build,
    /// `plus`.
    Plus,
    /// `grip`.
    Grip,
    /// `back`.
    Back,
    /// `forward`.
    Forward,
    /// `chevron-right`.
    ChevronRight,
    /// `chevron-down`.
    ChevronDown,
    /// `terminal`.
    Terminal,
    /// `hammer`.
    Hammer,
    /// `refresh`.
    Refresh,
    /// `up`.
    Up,
    /// `down`.
    Down,
    /// `prompts`.
    Prompts,
    /// `plug`.
    Plug,
    /// `wrench`.
    Wrench,
    /// `file`.
    File,
    /// `bench`.
    Bench,
    /// `select`.
    Select,
    /// `swap`.
    Swap,
    /// `edit`.
    Edit,
    /// `open`.
    Open,
    /// `filter`.
    Filter,
    /// `fork`.
    Fork,
    /// `sparkles`.
    Sparkles,
}

impl Icon {
    /// Every icon, in the order `web/app.js` defines them.
    pub const ALL: [Icon; 47] = [
        Icon::Send,
        Icon::Eye,
        Icon::EyeOff,
        Icon::Trophy,
        Icon::Play,
        Icon::Pause,
        Icon::Step,
        Icon::Restart,
        Icon::Stop,
        Icon::Settings,
        Icon::Copy,
        Icon::Check,
        Icon::Compose,
        Icon::Panel,
        Icon::Archive,
        Icon::Star,
        Icon::Sun,
        Icon::Moon,
        Icon::Search,
        Icon::X,
        Icon::Equal,
        Icon::Trash,
        Icon::Share,
        Icon::Build,
        Icon::Plus,
        Icon::Grip,
        Icon::Back,
        Icon::Forward,
        Icon::ChevronRight,
        Icon::ChevronDown,
        Icon::Terminal,
        Icon::Hammer,
        Icon::Refresh,
        Icon::Up,
        Icon::Down,
        Icon::Prompts,
        Icon::Plug,
        Icon::Wrench,
        Icon::File,
        Icon::Bench,
        Icon::Select,
        Icon::Swap,
        Icon::Edit,
        Icon::Open,
        Icon::Filter,
        Icon::Fork,
        Icon::Sparkles,
    ];

    /// The page's name.
    pub fn name(self) -> &'static str {
        match self {
            Icon::Send => "send",
            Icon::Eye => "eye",
            Icon::EyeOff => "eye-off",
            Icon::Trophy => "trophy",
            Icon::Play => "play",
            Icon::Pause => "pause",
            Icon::Step => "step",
            Icon::Restart => "restart",
            Icon::Stop => "stop",
            Icon::Settings => "settings",
            Icon::Copy => "copy",
            Icon::Check => "check",
            Icon::Compose => "compose",
            Icon::Panel => "panel",
            Icon::Archive => "archive",
            Icon::Star => "star",
            Icon::Sun => "sun",
            Icon::Moon => "moon",
            Icon::Search => "search",
            Icon::X => "x",
            Icon::Equal => "equal",
            Icon::Trash => "trash",
            Icon::Share => "share",
            Icon::Build => "build",
            Icon::Plus => "plus",
            Icon::Grip => "grip",
            Icon::Back => "back",
            Icon::Forward => "forward",
            Icon::ChevronRight => "chevron-right",
            Icon::ChevronDown => "chevron-down",
            Icon::Terminal => "terminal",
            Icon::Hammer => "hammer",
            Icon::Refresh => "refresh",
            Icon::Up => "up",
            Icon::Down => "down",
            Icon::Prompts => "prompts",
            Icon::Plug => "plug",
            Icon::Wrench => "wrench",
            Icon::File => "file",
            Icon::Bench => "bench",
            Icon::Select => "select",
            Icon::Swap => "swap",
            Icon::Edit => "edit",
            Icon::Open => "open",
            Icon::Filter => "filter",
            Icon::Fork => "fork",
            Icon::Sparkles => "sparkles",
        }
    }

    /// The drawing's elements, as the page writes them.
    pub fn glyph(self) -> &'static str {
        match self {
            Icon::Send => r#"<path d="m5 12 7-7 7 7" /><path d="M12 19V5" />"#,
            Icon::Eye => {
                r#"<path d="M2.06 12.35a1 1 0 0 1 0-.7 10.75 10.75 0 0 1 19.88 0 1 1 0 0 1 0 .7 10.75 10.75 0 0 1-19.88 0" /><circle cx="12" cy="12" r="3" />"#
            }
            Icon::EyeOff => {
                r#"<path d="M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68" /><path d="M6.61 6.61A13.53 13.53 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61" /><path d="M14.08 14.08a3 3 0 1 1-4.16-4.16" /><path d="m2 2 20 20" />"#
            }
            Icon::Trophy => {
                r#"<path d="M6 9H4.5a2.5 2.5 0 0 1 0-5H6" /><path d="M18 9h1.5a2.5 2.5 0 0 0 0-5H18" /><path d="M4 22h16" /><path d="M10 14.66V17c0 .55-.47.98-.97 1.21C7.85 18.75 7 20.24 7 22" /><path d="M14 14.66V17c0 .55.47.98.97 1.21C16.15 18.75 17 20.24 17 22" /><path d="M18 2H6v7a6 6 0 0 0 12 0V2Z" />"#
            }
            Icon::Play => r#"<path d="M7 4.5v15l12-7.5z" fill="currentColor" stroke="none" />"#,
            Icon::Pause => {
                r#"<rect x="6.5" y="5" width="3.5" height="14" rx="1" fill="currentColor" stroke="none" /><rect x="14" y="5" width="3.5" height="14" rx="1" fill="currentColor" stroke="none" />"#
            }
            Icon::Step => {
                r#"<path d="M5 5v14l10-7z" fill="currentColor" stroke="none" /><path d="M18.5 5v14" />"#
            }
            Icon::Restart => r#"<path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 3v5h5" />"#,
            Icon::Stop => {
                r#"<rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor" stroke="none" />"#
            }
            Icon::Settings => {
                r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" /><circle cx="12" cy="12" r="3" />"#
            }
            Icon::Copy => {
                r#"<rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />"#
            }
            Icon::Check => r#"<path d="m5 12 4 4L19 6" />"#,
            Icon::Compose => {
                r#"<path d="M12 3.5H6.5a3 3 0 0 0-3 3v11a3 3 0 0 0 3 3h11a3 3 0 0 0 3-3V12" /><path d="M18.4 3.1a1.9 1.9 0 0 1 2.7 2.7l-7.6 7.6a2 2 0 0 1-.9.5l-2.6.7.7-2.6a2 2 0 0 1 .5-.9z" />"#
            }
            Icon::Panel => {
                r#"<rect x="3" y="3" width="18" height="18" rx="2" /><path d="M9 3v18" />"#
            }
            Icon::Archive => {
                r#"<path d="M4 7h16" /><path d="M5 7v12h14V7" /><path d="M3 3h18v4H3z" /><path d="M10 11h4" />"#
            }
            Icon::Star => {
                r#"<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-2.9-5.6 2.9 1.1-6.2L3 9.6l6.2-.9z" />"#
            }
            Icon::Sun => {
                r#"<circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.93 4.93l1.42 1.42M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.42-1.42M17.66 6.34l1.41-1.41" />"#
            }
            Icon::Moon => r#"<path d="M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8Z" />"#,
            Icon::Search => r#"<circle cx="11" cy="11" r="7" /><path d="m20 20-4-4" />"#,
            Icon::X => r#"<path d="m6 6 12 12M18 6 6 18" />"#,
            Icon::Equal => r#"<path d="M5 9h14" /><path d="M5 15h14" />"#,
            Icon::Trash => {
                r#"<path d="M3 6h18" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /><path d="M10 11v6" /><path d="M14 11v6" />"#
            }
            Icon::Share => {
                r#"<circle cx="18" cy="5" r="2.5" /><circle cx="6" cy="12" r="2.5" /><circle cx="18" cy="19" r="2.5" /><path d="m8.2 10.8 7.6-4.4M8.2 13.2l7.6 4.4" />"#
            }
            Icon::Build => {
                r#"<path d="M4 8.5 12 4l8 4.5-8 4.5z" /><path d="m4 8.5 8 4.5 8-4.5M4 8.5V16l8 4 8-4V8.5M12 13v7" />"#
            }
            Icon::Plus => r#"<path d="M12 5v14M5 12h14" />"#,
            Icon::Grip => {
                r#"<circle cx="9" cy="4.5" r="1.6" fill="currentColor" stroke="none" /><circle cx="15" cy="4.5" r="1.6" fill="currentColor" stroke="none" /><circle cx="9" cy="9.5" r="1.6" fill="currentColor" stroke="none" /><circle cx="15" cy="9.5" r="1.6" fill="currentColor" stroke="none" /><circle cx="9" cy="14.5" r="1.6" fill="currentColor" stroke="none" /><circle cx="15" cy="14.5" r="1.6" fill="currentColor" stroke="none" /><circle cx="9" cy="19.5" r="1.6" fill="currentColor" stroke="none" /><circle cx="15" cy="19.5" r="1.6" fill="currentColor" stroke="none" />"#
            }
            Icon::Back => r#"<path d="m14 6-6 6 6 6" />"#,
            Icon::Forward => r#"<path d="m10 6 6 6-6 6" />"#,
            Icon::ChevronRight => r#"<path d="m9 6 6 6-6 6" />"#,
            Icon::ChevronDown => r#"<path d="m6 9 6 6 6-6" />"#,
            Icon::Terminal => r#"<path d="m5 7 4.5 5L5 17" /><path d="M12 17h7" />"#,
            Icon::Hammer => {
                r#"<path d="m14 5 5 5" /><path d="m12 7 4-4 5 5-4 4" /><path d="M14.5 9.5 6 18l-2 2" />"#
            }
            Icon::Refresh => {
                r#"<path d="M20 11a8 8 0 0 0-14.6-4.5L4 8" /><path d="M4 3v5h5" /><path d="M4 13a8 8 0 0 0 14.6 4.5L20 16" /><path d="M20 21v-5h-5" />"#
            }
            Icon::Up => r#"<path d="M12 19V5M6 11l6-6 6 6" />"#,
            Icon::Down => r#"<path d="M12 5v14M6 13l6 6 6-6" />"#,
            Icon::Prompts => {
                r#"<path d="M12 7v14" /><path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z" /><path d="M16 8h2M16 12h2M6 8h2M6 12h2" />"#
            }
            Icon::Plug => {
                r#"<path d="M9 2v5M15 2v5" /><path d="M6 7h12v4a6 6 0 0 1-12 0z" /><path d="M12 17v5" />"#
            }
            Icon::Wrench => {
                r#"<path d="M14.5 6.5a4 4 0 0 0-5-5l2.2 2.2-2.8 2.8-2.2-2.2a4 4 0 0 0 5 5L20 17.6 17.6 20l-8.3-8.3" />"#
            }
            Icon::File => r#"<path d="M6 3h8l4 4v14H6z" /><path d="M14 3v5h5M9 13h6M9 17h6" />"#,
            Icon::Bench => {
                r#"<path d="M3.5 20.5h17" /><path d="M5 20.5V12h4.5v8.5M9.5 20.5V6h5v14.5M14.5 20.5V14H19v6.5" />"#
            }
            Icon::Select => {
                r#"<path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3" /><path d="M11 4h2M11 20h2M4 11v2M20 11v2" />"#
            }
            Icon::Swap => {
                r#"<path d="M5 8h13.5M15 4.5 18.5 8 15 11.5" /><path d="M19 16H5.5M9 12.5 5.5 16 9 19.5" />"#
            }
            Icon::Edit => r#"<path d="M4 20h4L19 9l-4-4L4 16z" /><path d="m13.5 6.5 4 4" />"#,
            Icon::Open => {
                r#"<path d="M14 4h6v6M20 4l-8.5 8.5" /><path d="M18.5 13.5V18a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7.5a2 2 0 0 1 2-2h4.5" />"#
            }
            Icon::Filter => r#"<path d="M4 7h16M7 12h10M10 17h4" />"#,
            Icon::Fork => {
                r#"<circle cx="6" cy="5.5" r="2" /><circle cx="18" cy="5.5" r="2" /><circle cx="12" cy="18.5" r="2" /><path d="M6 7.5v1a3 3 0 0 0 3 3h6a3 3 0 0 0 3-3v-1M12 11.5v5" />"#
            }
            Icon::Sparkles => {
                r#"<path d="m12 3 1.2 3.3L16.5 7.5l-3.3 1.2L12 12l-1.2-3.3-3.3-1.2 3.3-1.2z" /><path d="m18 13 .8 2.2L21 16l-2.2.8L18 19l-.8-2.2L15 16l2.2-.8zM6 14l.6 1.4L8 16l-1.4.6L6 18l-.6-1.4L4 16l1.4-.6z" />"#
            }
        }
    }
}
/// The SVG document of an icon, as the page's `Icon` writes it (the colour
/// is GPUI's: it draws the alpha and paints it in the text colour).
pub fn icon_svg(icon: Icon) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
        icon.glyph()
    )
}

// bench agent: a favourite's star is filled (`.model-star.favorite svg {
// fill: currentColor }`).
/// An icon filled in the text colour as well as stroked, at `size` pixels:
/// a favourite's star.
pub fn icon_filled(icon: Icon, size: f32) -> impl IntoElement {
    Mask {
        path: format!("mcbench/icon-filled/{}.svg", icon.name()).into(),
        size: px(size),
    }
}

/// The asset path GPUI loads an icon from.
fn icon_path(icon: Icon) -> SharedString {
    format!("mcbench/icon/{}.svg", icon.name()).into()
}

/// An icon at `size` pixels in the current text colour (the page's
/// `<Icon name size>`). It adds nothing to the accessibility tree.
pub fn icon(icon: Icon, size: f32) -> impl IntoElement {
    Mask {
        path: icon_path(icon),
        size: px(size),
    }
}

/// An SVG drawn as a mask in the text colour its ancestors set. GPUI's
/// `svg()` paints only with a colour set on itself (it inherits none, and
/// draws nothing without one: the blank buttons of focus-03.png in the
/// review); this reads the inherited text style when it is laid out.
#[derive(IntoElement)]
struct Mask {
    path: SharedString,
    size: Pixels,
}

impl RenderOnce for Mask {
    fn render(self, window: &mut gpui_kit::Window, _: &mut gpui_kit::App) -> impl IntoElement {
        let colour = window.text_style().color;
        svg()
            .path(self.path)
            .size(self.size)
            .flex_none()
            .text_color(colour)
    }
}

pub use crate::state::vendor::Logo;

impl Logo {
    /// The page's file (`web/icons/<name>.svg`).
    fn file(self) -> &'static [u8] {
        match self {
            Logo::Anthropic => include_bytes!("../../assets/logos/anthropic.svg"),
            Logo::Claude => include_bytes!("../../assets/logos/claude.svg"),
            Logo::Codex => include_bytes!("../../assets/logos/codex.svg"),
            Logo::Cursor => include_bytes!("../../assets/logos/cursor.svg"),
            Logo::Deepseek => include_bytes!("../../assets/logos/deepseek.svg"),
            Logo::Gemini => include_bytes!("../../assets/logos/gemini.svg"),
            Logo::Groq => include_bytes!("../../assets/logos/groq.svg"),
            Logo::Meta => include_bytes!("../../assets/logos/meta.svg"),
            Logo::Mistral => include_bytes!("../../assets/logos/mistral.svg"),
            Logo::Moonshot => include_bytes!("../../assets/logos/moonshot.svg"),
            Logo::Openai => include_bytes!("../../assets/logos/openai.svg"),
            Logo::Openrouter => include_bytes!("../../assets/logos/openrouter.svg"),
            Logo::Qwen => include_bytes!("../../assets/logos/qwen.svg"),
            Logo::Xai => include_bytes!("../../assets/logos/xai.svg"),
        }
    }

    fn path(self) -> SharedString {
        format!("mcbench/logo/{}.svg", self.name()).into()
    }
}

pub use super::theme::{Brand, brand};

/// A gradient logo as one SVG: the brand's gradient over the logo's box,
/// masked by the logo's shape (the page's `.vlogo` mask over a gradient
/// background). CSS's `<angle>deg` on a square runs from the corner (or
/// edge) the angle names to the opposite one.
pub fn gradient_svg(logo: Logo, angle: f32, stops: &[(&str, f32)]) -> String {
    let file = std::str::from_utf8(logo.file()).expect("the logos are UTF-8");
    let open = file.find('>').expect("an <svg> element") + 1;
    let close = file.rfind("</svg>").expect("an </svg>");
    let inner = &file[open..close];
    let (x1, y1, x2, y2) = match angle as i32 {
        135 => (0., 0., 1., 1.),
        180 => (0., 0., 0., 1.),
        other => panic!("no gradient direction for {other}deg"),
    };
    let stops: String = stops
        .iter()
        .map(|(colour, at)| format!(r#"<stop offset="{at}" stop-color="{colour}"/>"#))
        .collect();
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 24 24"><defs><linearGradient id="g" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}">{stops}</linearGradient><mask id="m"><g fill="white" color="white" fill-rule="evenodd">{inner}</g></mask></defs><rect width="24" height="24" fill="url(#g)" mask="url(#m)"/></svg>"#
    )
}

fn gradient_image(logo: Logo) -> Arc<Image> {
    static IMAGES: OnceLock<Vec<(Logo, Arc<Image>)>> = OnceLock::new();
    IMAGES
        .get_or_init(|| {
            Logo::ALL
                .iter()
                .filter_map(|&logo| match brand(logo) {
                    Brand::Gradient(angle, stops) => Some((
                        logo,
                        Arc::new(Image::from_bytes(
                            ImageFormat::Svg,
                            gradient_svg(logo, angle, stops).into_bytes(),
                        )),
                    )),
                    _ => None,
                })
                .collect()
        })
        .iter()
        .find(|(l, _)| *l == logo)
        .map(|(_, image)| image.clone())
        .expect("a gradient logo has its picture")
}

/// A logo at `size` pixels (the page's `<Logo name size>`): in its brand
/// colour, or the current text colour. Decoration only, as an icon.
pub fn logo(logo: Logo, size: f32) -> gpui_kit::AnyElement {
    let size: Pixels = px(size);
    match brand(logo) {
        Brand::Text => Mask {
            path: logo.path(),
            size,
        }
        .into_any_element(),
        Brand::Solid(hex) => svg()
            .path(logo.path())
            .size(size)
            .flex_none()
            .text_color(colour::brand(hex))
            .into_any_element(),
        Brand::Gradient(..) => div()
            .size(size)
            .flex_none()
            .child(img(gradient_image(logo)).size(size))
            .into_any_element(),
    }
}

/// The app's own assets (icons and logos) in front of gpui-kit's.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some(name) = path
            .strip_prefix("mcbench/icon/")
            .and_then(|p| p.strip_suffix(".svg"))
        {
            let icon = Icon::ALL
                .into_iter()
                .find(|i| i.name() == name)
                .unwrap_or_else(|| panic!("no icon {name}"));
            return Ok(Some(Cow::Owned(icon_svg(icon).into_bytes())));
        }
        // bench agent: the filled icons.
        if let Some(name) = path
            .strip_prefix("mcbench/icon-filled/")
            .and_then(|p| p.strip_suffix(".svg"))
        {
            let icon = Icon::ALL
                .into_iter()
                .find(|i| i.name() == name)
                .unwrap_or_else(|| panic!("no icon {name}"));
            let svg = icon_svg(icon).replace(r#"fill="none""#, r#"fill="currentColor""#);
            return Ok(Some(Cow::Owned(svg.into_bytes())));
        }
        if let Some(name) = path
            .strip_prefix("mcbench/logo/")
            .and_then(|p| p.strip_suffix(".svg"))
        {
            let logo = Logo::ALL
                .into_iter()
                .find(|l| l.name() == name)
                .unwrap_or_else(|| panic!("no logo {name}"));
            return Ok(Some(Cow::Borrowed(logo.file())));
        }
        // pane agent: BlueMap's control bar (`view::world::bluemap`).
        if let Some(name) = path.strip_prefix(super::world::bluemap::PREFIX) {
            let svg = super::world::bluemap::asset(name)
                .unwrap_or_else(|| panic!("no BlueMap icon {name}"));
            return Ok(Some(Cow::Owned(svg.into_bytes())));
        }
        if path == MARK_WIRE_PATH {
            return Ok(Some(Cow::Borrowed(MARK_WIRE.as_bytes())));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}

/// The app's mark (`AppMark`, the same drawing as `web/icons/block.svg`):
/// the wire cube in the text colour, and the lit block inside it in its own
/// colours with its glow.
pub fn app_mark(size: f32) -> impl IntoElement {
    static BLOCK: OnceLock<Arc<Image>> = OnceLock::new();
    let block = BLOCK
        .get_or_init(|| {
            Arc::new(Image::from_bytes(
                ImageFormat::Svg,
                MARK_BLOCK.as_bytes().to_vec(),
            ))
        })
        .clone();
    div()
        .relative()
        .size(px(size))
        .flex_none()
        .child(div().absolute().child(Mask {
            path: MARK_WIRE_PATH.into(),
            size: px(size),
        }))
        .child(img(block).absolute().size(px(size)))
}

const MARK_WIRE_PATH: &str = "mcbench/mark-wire.svg";

/// `AppMark`'s wire, the part drawn in the text colour.
pub const MARK_WIRE: &str = include_str!("../../assets/mark-wire.svg");

/// `AppMark`'s lit block, drawn in its own colours (`web/icons/block.svg`'s).
pub const MARK_BLOCK: &str = include_str!("../../assets/mark-block.svg");
