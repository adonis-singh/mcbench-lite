//! The design tokens of `web/style.css` (inventory: `style-viewer-routes.md`
//! 1.2 and 1.3), the one place a colour is written down. Everything else
//! names a token here: [`colour`] for the colours, [`shadow`] for the
//! shadows, [`radius`] and [`TextKind`] for shapes and type.
//!
//! The theme is dark or light, as the page's `data-theme` (`web/theme.js`):
//! dark unless the stored preference says light. The page never consults the
//! system's appearance (it has no `prefers-color-scheme` rule and no
//! "system" choice), so neither does the app: [`set_mode`] is called with
//! the stored preference, and a change redraws every window.
//!
//! Fonts are what the page's stacks resolve to on each platform, with the
//! app's own last entry ([`super::fonts`]). Letter spacing (`-0.2px` on the
//! wordmark, for example) has no GPUI counterpart: the value is kept in
//! [`TextStyle::tracking`] and not drawn.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::{
    BoxShadow, FontWeight, Hsla, Pixels, Rgba, SharedString, Styled, point, px, relative,
};

/// Dark (the default) or light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `data-theme="dark"`.
    Dark,
    /// `data-theme="light"`.
    Light,
}

static LIGHT: AtomicBool = AtomicBool::new(false);

/// Sets the theme every token reads from; the caller redraws the windows.
pub fn set_mode(mode: Mode) {
    LIGHT.store(mode == Mode::Light, Ordering::SeqCst);
}

/// The theme in force.
pub fn mode() -> Mode {
    if LIGHT.load(Ordering::SeqCst) {
        Mode::Light
    } else {
        Mode::Dark
    }
}

const fn hex(value: u32) -> Rgba {
    Rgba {
        r: ((value >> 16) & 0xff) as f32 / 255.,
        g: ((value >> 8) & 0xff) as f32 / 255.,
        b: (value & 0xff) as f32 / 255.,
        a: 1.,
    }
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Rgba {
    Rgba {
        r: r as f32 / 255.,
        g: g as f32 / 255.,
        b: b as f32 / 255.,
        a,
    }
}

/// The colour and surface tokens (S:676-741).
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// `--bedrock`: the window, the sidebar.
    pub bedrock: Rgba,
    /// `--cave`: the chat column.
    pub cave: Rgba,
    /// `--well`: the composer box, popovers, inline code.
    pub well: Rgba,
    /// `--raised`: buttons, the user's message, the active row.
    pub raised: Rgba,
    /// `--popover`: tooltips, the command panel, the share panel.
    pub popover: Rgba,
    /// `--bevel-light`: hover borders.
    pub bevel_light: Rgba,
    /// `--bone`: primary text, the primary button's fill.
    pub bone: Rgba,
    /// `--ash`: secondary text.
    pub ash: Rgba,
    /// `--dim-text`: tertiary text, placeholders, icons.
    pub dim_text: Rgba,
    /// `--gold`: the accent (near white in dark): the focus outline, toggles.
    pub gold: Rgba,
    /// `--star`: favourite stars, warnings.
    pub star: Rgba,
    /// `--grass`.
    pub grass: Rgba,
    /// `--done`: a bench's done status.
    pub done: Rgba,
    /// `--waiting`: the unread chat dot.
    pub waiting: Rgba,
    /// `--redstone`: error borders and tints.
    pub redstone: Rgba,
    /// `--hover`: row and ghost-control hover.
    pub hover: Rgba,
    /// `--hover-well`.
    pub hover_well: Rgba,
    /// `--note-text`.
    pub note_text: Rgba,
    /// `--reason-text`.
    pub reason_text: Rgba,
    /// `--err-text`: error text.
    pub err_text: Rgba,
    /// `--err-soft`.
    pub err_soft: Rgba,
    /// `--term-text`: bash output.
    pub term_text: Rgba,
    /// `--line`: every hairline.
    pub line: Rgba,
    /// `--input-line`.
    pub input_line: Rgba,
    /// `--sky-high`.
    pub sky_high: Rgba,
    /// `--tk-c`: comments in code.
    pub tk_c: Rgba,
    /// `--tk-f`: functions.
    pub tk_f: Rgba,
    /// `--tk-n`: numbers.
    pub tk_n: Rgba,
    /// `--tk-s`: strings.
    pub tk_s: Rgba,
    /// `--tk-k`: keywords.
    pub tk_k: Rgba,
    /// `--diff-plus`.
    pub diff_plus: Rgba,
    /// The text selection (`::selection`, `--waiting` at 35%).
    pub selection: Rgba,
    /// The scrim behind the narrow window's drawer (`rgba(0, 0, 0, 0.55)`).
    pub scrim: Rgba,
    /// `#fff`: the danger button's text.
    pub white: Rgba,
    /// `#000`: the viewer's background, in both themes.
    pub viewer: Rgba,
}

/// The dark theme (S:676-710, S:8-20).
pub const DARK: Palette = Palette {
    bedrock: hex(0x0a0a0a),
    cave: hex(0x0d0d0d),
    well: hex(0x111111),
    raised: hex(0x1a1a1a),
    popover: hex(0x141414),
    bevel_light: hex(0x2a2a2a),
    bone: hex(0xf5f5f5),
    ash: hex(0xa3a3a3),
    dim_text: hex(0x7c7c7c),
    gold: hex(0xededed),
    star: hex(0xf0b54a),
    grass: hex(0x7fbf6a),
    done: hex(0x3ecf6b),
    waiting: hex(0x6cb4ff),
    redstone: hex(0xeb6a60),
    hover: rgba(255, 255, 255, 0.06),
    hover_well: rgba(255, 255, 255, 0.04),
    note_text: hex(0xa3a3a3),
    reason_text: hex(0x8a8a8a),
    err_text: hex(0xf3a7a1),
    err_soft: hex(0xcc827c),
    term_text: hex(0xa8a8a8),
    line: rgba(255, 255, 255, 0.06),
    input_line: rgba(255, 255, 255, 0.08),
    sky_high: hex(0x6f9fdd),
    tk_c: hex(0x626b78),
    tk_f: hex(0x8fbde8),
    tk_n: hex(0x6f9fdd),
    tk_s: hex(0x7fbf6a),
    tk_k: hex(0xededed),
    diff_plus: hex(0x7fbf6a),
    selection: rgba(0x6c, 0xb4, 0xff, 0.35),
    scrim: rgba(0, 0, 0, 0.55),
    white: hex(0xffffff),
    viewer: hex(0x000000),
};

/// The light theme (S:712-741, S:23).
pub const LIGHT_PALETTE: Palette = Palette {
    bedrock: hex(0xfafafa),
    cave: hex(0xffffff),
    well: hex(0xffffff),
    raised: hex(0xf4f4f5),
    popover: hex(0xffffff),
    bevel_light: hex(0xffffff),
    bone: hex(0x27272a),
    ash: hex(0x52525b),
    dim_text: hex(0x71717a),
    gold: hex(0x18181b),
    star: hex(0xc9820f),
    grass: hex(0x4f8a35),
    done: hex(0x1f9d4c),
    waiting: hex(0x2b7de9),
    redstone: hex(0xd14a42),
    hover: rgba(0, 0, 0, 0.05),
    hover_well: rgba(0, 0, 0, 0.03),
    note_text: hex(0x52525b),
    reason_text: hex(0x71717a),
    err_text: hex(0xa8322d),
    err_soft: hex(0xb9544e),
    term_text: hex(0x52525b),
    line: hex(0xe4e4e7),
    input_line: hex(0xd4d4d8),
    sky_high: hex(0x6f9fdd),
    tk_c: hex(0x918872),
    tk_f: hex(0x23639c),
    tk_n: hex(0x2f6bb0),
    tk_s: hex(0x4a7a33),
    tk_k: hex(0xa06a10),
    diff_plus: hex(0x4a7c32),
    selection: rgba(0x2b, 0x7d, 0xe9, 0.35),
    scrim: rgba(0, 0, 0, 0.55),
    white: hex(0xffffff),
    viewer: hex(0x000000),
};

/// The palette in force.
pub fn palette() -> &'static Palette {
    palette_for(mode())
}

/// A theme's palette.
pub fn palette_for(mode: Mode) -> &'static Palette {
    match mode {
        Mode::Dark => &DARK,
        Mode::Light => &LIGHT_PALETTE,
    }
}

/// `color-mix(in srgb, a p%, b)`: `a` weighted `p` (0 to 1) over `b`.
pub fn mix(a: Rgba, p: f32, b: Rgba) -> Rgba {
    Rgba {
        r: a.r * p + b.r * (1. - p),
        g: a.g * p + b.g * (1. - p),
        b: a.b * p + b.b * (1. - p),
        a: a.a * p + b.a * (1. - p),
    }
}

/// `color-mix(in srgb, a p%, transparent)`: `a` at `p` of its opacity.
pub fn fade(a: Rgba, p: f32) -> Rgba {
    Rgba { a: a.a * p, ..a }
}

/// The colours by name, as the views use them (each reads the palette in
/// force).
pub mod colour {
    use super::{fade, mix, palette};
    use gpui_kit::Hsla;

    macro_rules! token {
        ($($(#[$doc:meta])* $name:ident => $field:ident;)*) => {
            $($(#[$doc])* pub fn $name() -> Hsla { palette().$field.into() })*
        };
    }
    token! {
        /// `--bedrock`.
        bedrock => bedrock;
        /// `--cave`.
        cave => cave;
        /// `--well`.
        well => well;
        /// `--raised`.
        raised => raised;
        /// `--popover`.
        popover => popover;
        /// `--bevel-light`.
        bevel_light => bevel_light;
        /// `--bone`.
        bone => bone;
        /// `--ash`.
        ash => ash;
        /// `--dim-text`.
        dim => dim_text;
        /// `--gold`: the focus ring.
        gold => gold;
        /// `--star`.
        star => star;
        /// `--grass`.
        grass => grass;
        /// `--done`.
        done => done;
        /// `--waiting`.
        waiting => waiting;
        /// `--redstone`.
        redstone => redstone;
        /// `--hover`.
        hover => hover;
        /// `--hover-well`.
        hover_well => hover_well;
        /// `--note-text`.
        note => note_text;
        /// `--reason-text`.
        reason => reason_text;
        /// `--err-text`.
        err => err_text;
        /// `--err-soft`.
        err_soft => err_soft;
        /// `--term-text`.
        term => term_text;
        /// `--line`.
        line => line;
        /// `--input-line`.
        input_line => input_line;
        /// `--sky-high`.
        sky_high => sky_high;
        /// `--diff-plus`.
        diff_plus => diff_plus;
        /// The text selection.
        selection => selection;
        /// The drawer's scrim.
        scrim => scrim;
        /// `#fff`.
        white => white;
        /// `#000`: the viewer.
        viewer => viewer;
    }

    /// Transparent (a reserved border that draws nothing).
    pub fn none() -> Hsla {
        fade(palette().bone, 0.).into()
    }
    /// `color-mix(in srgb, var(--bone) 3.5%, transparent)`: a row's hover.
    pub fn faint() -> Hsla {
        fade(palette().bone, 0.035).into()
    }
    /// `color-mix(in srgb, var(--bone) 82%, transparent)`: a tool's action.
    pub fn bone_82() -> Hsla {
        fade(palette().bone, 0.82).into()
    }
    // bench agent: a picked vote's tint.
    /// `color-mix(in srgb, var(--done) 10%, transparent)`: a picked vote.
    pub fn done_fill() -> Hsla {
        fade(palette().done, 0.10).into()
    }
    // bench agent: the prompt tags' colours (`TAG_COLORS`, the same in both
    // themes).
    /// `TAG_COLORS[index]`: a tag's colour (`state::prompts::tag_color`).
    pub fn tag(index: usize) -> Hsla {
        const TAGS: [(u8, u8, u8); 8] = [
            (0x7d, 0xa8, 0x69),
            (0xc9, 0xa2, 0x5e),
            (0x6f, 0x9a, 0xa8),
            (0x9a, 0x7f, 0xae),
            (0xa8, 0x78, 0x5f),
            (0xa8, 0x6f, 0x7e),
            (0x8f, 0xa8, 0x6f),
            (0x6f, 0x7f, 0xa8),
        ];
        let (r, g, b) = TAGS[index];
        super::rgba(r, g, b, 1.).into()
    }
    /// `color-mix(in srgb, var(--dim-text) 82%, transparent)`: a tool's detail.
    pub fn dim_82() -> Hsla {
        fade(palette().dim_text, 0.82).into()
    }
    // chat (stage four's chat agent)
    /// `color-mix(in srgb, var(--dim-text) 60%, transparent)`: a done
    /// task's strike (`.plan-card li.done`).
    pub fn dim_60() -> Hsla {
        fade(palette().dim_text, 0.6).into()
    }
    /// `color-mix(in srgb, var(--dim-text) 70%, transparent)`: the working
    /// line (`.ticker`).
    pub fn dim_70() -> Hsla {
        fade(palette().dim_text, 0.7).into()
    }
    // pane agent: the 3D pane's own colours (BlueMap's bar and the
    // bridge's overlays: `web/bluemap-bridge.js` 51-72, 958-989).
    /// `rgba(12, 12, 12, 0.86)`: a build label's pill (`.mcbench-tag`).
    pub fn live_tag() -> Hsla {
        gpui_kit::Rgba {
            r: 12. / 255.,
            g: 12. / 255.,
            b: 12. / 255.,
            a: 0.86,
        }
        .into()
    }
    /// `#f5f5f5`: a build label's text.
    pub fn live_tag_text() -> Hsla {
        gpui_kit::rgb(0xf5f5f5).into()
    }
    /// `rgba(12, 12, 12, 0.62)`: the player hint's pill.
    pub fn live_hud() -> Hsla {
        gpui_kit::Rgba {
            r: 12. / 255.,
            g: 12. / 255.,
            b: 12. / 255.,
            a: 0.62,
        }
        .into()
    }
    /// `#d8d3c8`: the player hint's text.
    pub fn live_hud_text() -> Hsla {
        gpui_kit::rgb(0xd8d3c8).into()
    }
    /// `#ffd7a8`: a player note's text.
    pub fn live_note_text() -> Hsla {
        gpui_kit::rgb(0xffd7a8).into()
    }
    /// White: the crosshair, the centre mark.
    pub fn live_mark() -> Hsla {
        gpui_kit::rgb(0xffffff).into()
    }
    /// `#f5f5f5`: a selection marker's ring (`.mcbench-icon`,
    /// web/bluemap-bridge.js:71).
    pub fn live_mark_ring() -> Hsla {
        gpui_kit::rgb(0xf5f5f5).into()
    }
    /// `rgba(12, 12, 12, 0.86)`: an empty selection marker
    /// (`.mcbench-icon.empty`, web/bluemap-bridge.js:72).
    pub fn live_mark_empty() -> Hsla {
        gpui_kit::Rgba {
            r: 12. / 255.,
            g: 12. / 255.,
            b: 12. / 255.,
            a: 0.86,
        }
        .into()
    }
    /// BlueMap's bar's buttons (`--theme-bg` of BlueMap 5.22's dark theme,
    /// `.theme-dark` in its baked `index-BgiqB2rB.css`): `#181818`.
    pub fn live_bar() -> Hsla {
        gpui_kit::rgb(0x181818).into()
    }
    /// A hovered button (`--theme-bg-hover`): `#222`.
    pub fn live_bar_hover() -> Hsla {
        gpui_kit::rgb(0x222222).into()
    }
    /// An active button and the separators (`--theme-bg-light`): `#444`.
    pub fn live_bar_light() -> Hsla {
        gpui_kit::rgb(0x444444).into()
    }
    /// The bar's text (`--theme-fg`): `#fff`.
    pub fn live_bar_text() -> Hsla {
        gpui_kit::rgb(0xffffff).into()
    }
    /// Its icons and labels (`--theme-fg-light`): `#aaa`.
    pub fn live_bar_dim() -> Hsla {
        gpui_kit::rgb(0xaaaaaa).into()
    }
    /// `rgba(0, 0, 0, 0.78)`: the lightbox's backdrop (`.lightbox`).
    pub fn lightbox_scrim() -> Hsla {
        gpui_kit::Rgba {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 0.78,
        }
        .into()
    }
    /// `color-mix(in srgb, var(--bone) 28%, transparent)`: the border of the
    /// message the composer edits (`.user-block.editing .msg.user`).
    pub fn bone_28() -> Hsla {
        fade(palette().bone, 0.28).into()
    }
    /// `color-mix(in srgb, var(--bone) 24%, transparent)`: the composer
    /// box's border while it holds focus.
    pub fn bone_24() -> Hsla {
        fade(palette().bone, 0.24).into()
    }
    /// `color-mix(in srgb, var(--bone) 58%, transparent)`: the mode word.
    pub fn mode_word() -> Hsla {
        fade(palette().bone, 0.58).into()
    }
    /// `color-mix(in srgb, var(--bone) 35%, transparent)`: the sidebar
    /// edge's hover line.
    pub fn edge_line() -> Hsla {
        fade(palette().bone, 0.35).into()
    }
    /// `color-mix(in srgb, var(--gold) 25%, transparent)`: the divider's
    /// hover glow.
    pub fn gold_glow() -> Hsla {
        fade(palette().gold, 0.25).into()
    }
    /// `color-mix(in srgb, var(--gold) 85%, white)`: the global
    /// `:focus-visible` outline.
    pub fn focus_outline() -> Hsla {
        mix(palette().gold, 0.85, palette().white).into()
    }
    /// `color-mix(in srgb, var(--bone) 14%, transparent)`: a switch's track
    /// when off.
    pub fn toggle_track() -> Hsla {
        fade(palette().bone, 0.14).into()
    }
    /// `color-mix(in srgb, var(--bone) 7%, transparent)`: a row icon's hover.
    pub fn bone_7() -> Hsla {
        fade(palette().bone, 0.07).into()
    }
    /// `color-mix(in srgb, var(--raised) 84%, var(--bone))`: a raised
    /// button's hover.
    pub fn raised_hover() -> Hsla {
        mix(palette().raised, 0.84, palette().bone).into()
    }
    /// `color-mix(in srgb, var(--bone) 88%, var(--ash))`: the primary
    /// button's hover.
    pub fn primary_hover() -> Hsla {
        mix(palette().bone, 0.88, palette().ash).into()
    }
    /// `color-mix(in srgb, var(--redstone) 28%, transparent)`: an error's
    /// border.
    pub fn error_border() -> Hsla {
        fade(palette().redstone, 0.28).into()
    }
    /// `color-mix(in srgb, var(--redstone) 8%, var(--well))`: an error's
    /// background.
    pub fn error_fill() -> Hsla {
        mix(palette().redstone, 0.08, palette().well).into()
    }
    /// `color-mix(in srgb, var(--redstone) 16%, transparent)`: the error
    /// dismiss button's hover.
    pub fn error_dismiss_hover() -> Hsla {
        fade(palette().redstone, 0.16).into()
    }
    // par-chat (stage four's parity pass)
    /// The composer's Stop (`.composer-controls > .stop`, S:1869-1878):
    /// `--redstone` mixed into `--well` at 10%, 17% on hover.
    pub fn stop_face(hover: bool) -> Hsla {
        mix(
            palette().redstone,
            if hover { 0.17 } else { 0.10 },
            palette().well,
        )
        .into()
    }
    /// Its border: `--redstone` mixed into `--line` at 25%, 45% on hover.
    pub fn stop_border(hover: bool) -> Hsla {
        mix(
            palette().redstone,
            if hover { 0.45 } else { 0.25 },
            palette().line,
        )
        .into()
    }
    /// Send's hover (`.send:hover:not(:disabled)`): `--bone` 88% with `--ash`.
    pub fn send_hover() -> Hsla {
        mix(palette().bone, 0.88, palette().ash).into()
    }
    /// `color-mix(in srgb, var(--redstone) 12%, transparent)`: the header's
    /// model warning (`.cap-warn`).
    pub fn cap_warn_fill() -> Hsla {
        fade(palette().redstone, 0.12).into()
    }
    /// `color-mix(in srgb, var(--redstone) 10%, transparent)`: Stop's fill.
    pub fn stop_fill() -> Hsla {
        fade(palette().redstone, 0.10).into()
    }
    /// `color-mix(in srgb, var(--err-text) 14%, transparent)`: an armed
    /// row icon.
    pub fn armed_fill() -> Hsla {
        fade(palette().err_text, 0.14).into()
    }
    // pickers: the composer's pickers and meters.
    /// `color-mix(in srgb, var(--gold) 10%, var(--raised))`: the chosen
    /// model's row (`.model-picker-row.selected`).
    pub fn picker_selected() -> Hsla {
        mix(palette().gold, 0.10, palette().raised).into()
    }
    /// `color-mix(in srgb, var(--bone) 26%, transparent)`: the resolution
    /// slider's focus ring.
    pub fn slider_ring() -> Hsla {
        fade(palette().bone, 0.26).into()
    }
    /// The context meter's track (`.context-meter`):
    /// `color-mix(in srgb, var(--bone) 14%, transparent)`.
    pub fn meter_track() -> Hsla {
        fade(palette().bone, 0.14).into()
    }
    /// The context meter's fill: `--ash`, `--star` when high.
    pub fn meter_fill(high: bool) -> Hsla {
        if high { palette().star } else { palette().ash }.into()
    }
    /// The limit chip's text: `--star`, `--err-text` when the limit was
    /// reached (`.limit-chip`, `.rejected`).
    pub fn limit_text(rejected: bool) -> Hsla {
        if rejected {
            palette().err_text
        } else {
            palette().star
        }
        .into()
    }
    /// The limit chip's fill: `--star` at 14%, `--redstone` at 16% when the
    /// limit was reached.
    pub fn limit_fill(rejected: bool) -> Hsla {
        if rejected {
            fade(palette().redstone, 0.16)
        } else {
            fade(palette().star, 0.14)
        }
        .into()
    }
    /// A brand colour of `BRAND` (`#rrggbb`, from [`super::brand`]).
    pub fn brand(hex: &str) -> Hsla {
        let value = u32::from_str_radix(hex.trim_start_matches('#'), 16)
            .unwrap_or_else(|e| panic!("the brand colour {hex} is not #rrggbb: {e}"));
        super::hex(value).into()
    }
    /// Code tokens (`--tk-*`): comment, string, number, keyword, function.
    pub fn token(kind: char) -> Hsla {
        let p = palette();
        match kind {
            'c' => p.tk_c,
            's' => p.tk_s,
            'n' => p.tk_n,
            'k' => p.tk_k,
            _ => p.tk_f,
        }
        .into()
    }
}

/// A logo's brand fill (`BRAND`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Brand {
    /// The text colour.
    Text,
    /// One colour, as `#rrggbb`.
    Solid(&'static str),
    /// A CSS `linear-gradient(<angle>deg, <stops>)`: the angle and each
    /// stop's colour and position (0 to 1).
    Gradient(f32, &'static [(&'static str, f32)]),
}

/// `BRAND`, `web/app.js:540-549`.
pub fn brand(logo: crate::state::vendor::Logo) -> Brand {
    use crate::state::vendor::Logo;
    match logo {
        Logo::Claude => Brand::Solid("#d97757"),
        Logo::Openrouter => Brand::Solid("#7072f5"),
        Logo::Groq => Brand::Solid("#f55036"),
        Logo::Deepseek => Brand::Solid("#4d6bfe"),
        Logo::Qwen => Brand::Gradient(135., &[("#7b6cf6", 0.), ("#5b4fe0", 1.)]),
        Logo::Gemini => Brand::Gradient(
            135.,
            &[("#4285f4", 0.15), ("#9b72cb", 0.55), ("#d96570", 0.9)],
        ),
        Logo::Meta => Brand::Gradient(135., &[("#0081fb", 0.), ("#0064e1", 1.)]),
        Logo::Mistral => {
            Brand::Gradient(180., &[("#ffd800", 0.), ("#ff8205", 0.5), ("#e10500", 1.)])
        }
        _ => Brand::Text,
    }
}

/// One layer of a CSS `box-shadow`.
#[derive(Clone, Copy, Debug)]
pub struct Layer {
    /// `inset`.
    pub inset: bool,
    /// The x offset.
    pub x: f32,
    /// The y offset.
    pub y: f32,
    /// The blur radius.
    pub blur: f32,
    /// The spread radius.
    pub spread: f32,
    /// The colour.
    pub colour: Rgba,
}

const fn layer(inset: bool, x: f32, y: f32, blur: f32, spread: f32, colour: Rgba) -> Layer {
    Layer {
        inset,
        x,
        y,
        blur,
        spread,
        colour,
    }
}

/// The shadow tokens of one theme (S:701, S:2759-2781).
#[derive(Clone, Copy, Debug)]
pub struct Shadows {
    /// `--highlight`.
    pub highlight: [Layer; 1],
    /// `--shadow-panel`.
    pub panel: [Layer; 2],
    /// `--glow-gold`.
    pub glow_gold: [Layer; 1],
    /// `--control-face`: the raised control's edge (no border).
    pub control_face: [Layer; 3],
    /// `--control-pressed`.
    pub control_pressed: [Layer; 2],
    /// `--primary-face`.
    pub primary_face: [Layer; 3],
    /// `--primary-pressed`.
    pub primary_pressed: [Layer; 1],
    /// `--field-face` (`0 0 0 1px var(--input-line)`).
    pub field_face: [Layer; 1],
}

/// The dark theme's shadows.
pub const DARK_SHADOWS: Shadows = Shadows {
    highlight: [layer(true, 0., 1., 0., 0., rgba(255, 255, 255, 0.05))],
    panel: [
        layer(false, 0., 0., 0., 1., rgba(255, 255, 255, 0.06)),
        layer(false, 0., 16., 48., 0., rgba(0, 0, 0, 0.6)),
    ],
    glow_gold: [layer(false, 0., 0., 0., 3., rgba(255, 255, 255, 0.08))],
    control_face: [
        layer(true, 0., 1., 0., 0., rgba(255, 255, 255, 0.07)),
        layer(false, 0., 0., 0., 1., rgba(255, 255, 255, 0.09)),
        layer(false, 0., 1., 2., 0., rgba(0, 0, 0, 0.55)),
    ],
    control_pressed: [
        layer(true, 0., 1., 2., 0., rgba(0, 0, 0, 0.35)),
        layer(false, 0., 0., 0., 1., rgba(255, 255, 255, 0.08)),
    ],
    primary_face: [
        layer(true, 0., 1., 0., 0., rgba(255, 255, 255, 0.9)),
        layer(true, 0., -1., 0., 0., rgba(0, 0, 0, 0.16)),
        layer(false, 0., 1., 3., 0., rgba(0, 0, 0, 0.5)),
    ],
    primary_pressed: [layer(true, 0., 1., 2., 0., rgba(0, 0, 0, 0.25))],
    field_face: [layer(false, 0., 0., 0., 1., rgba(255, 255, 255, 0.08))],
};

/// The light theme's shadows.
pub const LIGHT_SHADOWS: Shadows = Shadows {
    highlight: [layer(true, 0., -1., 0., 0., rgba(0, 0, 0, 0.04))],
    panel: [
        layer(false, 0., 0., 0., 1., rgba(0, 0, 0, 0.06)),
        layer(false, 0., 16., 48., 0., rgba(0, 0, 0, 0.12)),
    ],
    glow_gold: [layer(false, 0., 0., 0., 3., rgba(0, 0, 0, 0.08))],
    control_face: [
        layer(true, 0., -1., 0., 0., rgba(0, 0, 0, 0.04)),
        layer(false, 0., 0., 0., 1., rgba(0, 0, 0, 0.12)),
        layer(false, 0., 1., 2., 0., rgba(0, 0, 0, 0.06)),
    ],
    control_pressed: [
        layer(true, 0., 1., 2., 0., rgba(0, 0, 0, 0.1)),
        layer(false, 0., 0., 0., 1., rgba(0, 0, 0, 0.12)),
    ],
    primary_face: [
        layer(true, 0., 1., 0., 0., rgba(255, 255, 255, 0.16)),
        layer(false, 0., 0., 0., 1., rgba(0, 0, 0, 0.9)),
        layer(false, 0., 1., 2., 0., rgba(0, 0, 0, 0.2)),
    ],
    primary_pressed: [layer(true, 0., 1., 2., 0., rgba(0, 0, 0, 0.4))],
    field_face: [layer(false, 0., 0., 0., 1., hex(0xd4d4d8))],
};

/// The shadows by name, as GPUI box shadows.
pub mod shadow {
    use super::{DARK_SHADOWS, LIGHT_SHADOWS, Layer, Mode, Shadows, fade, mode, palette};
    use gpui_kit::{BoxShadow, Rgba};

    fn shadows() -> &'static Shadows {
        match mode() {
            Mode::Dark => &DARK_SHADOWS,
            Mode::Light => &LIGHT_SHADOWS,
        }
    }

    /// A CSS shadow list as GPUI's.
    pub fn of(layers: &[Layer]) -> Vec<BoxShadow> {
        layers.iter().map(|l| super::box_shadow(*l)).collect()
    }

    /// `--highlight`.
    pub fn highlight() -> Vec<BoxShadow> {
        of(&shadows().highlight)
    }
    /// `--shadow-panel`.
    pub fn panel() -> Vec<BoxShadow> {
        of(&shadows().panel)
    }
    /// `--glow-gold`.
    pub fn glow_gold() -> Vec<BoxShadow> {
        of(&shadows().glow_gold)
    }
    /// `--control-face`.
    pub fn control_face() -> Vec<BoxShadow> {
        of(&shadows().control_face)
    }
    /// `--control-pressed`.
    pub fn control_pressed() -> Vec<BoxShadow> {
        of(&shadows().control_pressed)
    }
    /// `--primary-face`.
    pub fn primary_face() -> Vec<BoxShadow> {
        of(&shadows().primary_face)
    }
    /// `--primary-pressed`.
    pub fn primary_pressed() -> Vec<BoxShadow> {
        of(&shadows().primary_pressed)
    }
    /// `--field-face`.
    pub fn field_face() -> Vec<BoxShadow> {
        of(&shadows().field_face)
    }
    /// `inset 0 0 0 1px var(--line)`: the quiet header action's ring, the
    /// sidebar search.
    pub fn line_ring() -> Vec<BoxShadow> {
        of(&[super::layer(true, 0., 0., 0., 1., palette().line)])
    }
    // bench agent: a picked vote's ring.
    /// `inset 0 0 0 1px color-mix(in srgb, var(--done) 45%, transparent)`:
    /// a picked vote (`.bench-vote.picked`).
    pub fn done_ring() -> Vec<BoxShadow> {
        of(&[super::layer(
            true,
            0.,
            0.,
            0.,
            1.,
            super::fade(palette().done, 0.45),
        )])
    }
    // par-chat (stage four's parity pass)
    /// `.composer-box`: `0 8px 30px rgba(0, 0, 0, 0.14)`; with focus inside
    /// (`:focus-within`) `0 8px 30px rgba(0, 0, 0, 0.16), 0 0 0 3px
    /// color-mix(in srgb, var(--bone) 5%, transparent)` (S:1388-1397).
    pub fn composer(focused: bool) -> Vec<BoxShadow> {
        if focused {
            of(&[
                super::layer(false, 0., 8., 30., 0., super::rgba(0, 0, 0, 0.16)),
                super::layer(false, 0., 0., 0., 3., fade(palette().bone, 0.05)),
            ])
        } else {
            of(&[super::layer(
                false,
                0.,
                8.,
                30.,
                0.,
                super::rgba(0, 0, 0, 0.14),
            )])
        }
    }
    /// `inset 0 -1px 0 var(--line)`: a line under a bar (`.pp-search`).
    pub fn line_below() -> Vec<BoxShadow> {
        of(&[super::layer(true, 0., -1., 0., 0., palette().line)])
    }
    /// `inset 0 1px 0 var(--line)`: a line over a bar (`.pp-foot`).
    pub fn line_above() -> Vec<BoxShadow> {
        of(&[super::layer(true, 0., 1., 0., 0., palette().line)])
    }
    /// `inset 0 0 0 1px var(--bevel-light)`: an armed quiet action.
    pub fn bevel_ring() -> Vec<BoxShadow> {
        of(&[super::layer(true, 0., 0., 0., 1., palette().bevel_light)])
    }
    /// The tooltip's shadow: `var(--highlight), 0 0 0 1px var(--line),
    /// 0 8px 24px rgba(0, 0, 0, 0.35)`.
    pub fn tooltip() -> Vec<BoxShadow> {
        let mut layers = shadows().highlight.to_vec();
        layers.push(super::layer(false, 0., 0., 0., 1., palette().line));
        layers.push(super::layer(
            false,
            0.,
            8.,
            24.,
            0.,
            Rgba {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 0.35,
            },
        ));
        of(&layers)
    }
    /// `.tag-menu` and the effort menus: `0 16px 42px rgba(0, 0, 0, 0.42)`.
    pub fn menu() -> Vec<BoxShadow> {
        of(&[super::layer(
            false,
            0.,
            16.,
            42.,
            0.,
            Rgba {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 0.42,
            },
        )])
    }
    /// The switch's thumb when off: `0 1px 2px rgba(0, 0, 0, 0.35)`.
    pub fn thumb() -> Vec<BoxShadow> {
        of(&[super::layer(
            false,
            0.,
            1.,
            2.,
            0.,
            Rgba {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 0.35,
            },
        )])
    }
    /// The divider's hover glow: `0 0 0 1px color-mix(var(--gold) 25%)`.
    pub fn divider_glow() -> Vec<BoxShadow> {
        of(&[super::layer(
            false,
            0.,
            0.,
            0.,
            1.,
            fade(palette().gold, 0.25),
        )])
    }
}

fn box_shadow(l: Layer) -> BoxShadow {
    BoxShadow {
        color: Hsla::from(l.colour),
        offset: point(px(l.x), px(l.y)),
        blur_radius: px(l.blur),
        spread_radius: px(l.spread),
        inset: l.inset,
    }
}

// sidebar agent: the sidebar's dots, search ring and the palette's scrim.
/// The sidebar's own colours and shadows (`style-viewer-routes.md` 1.5,
/// 1.16, 1.19).
pub mod sidebar {
    use super::{fade, layer, mix, palette, shadow};
    use gpui_kit::{BoxShadow, Hsla, Rgba};

    /// `conv-pulse`'s `from`: `color-mix(in srgb, var(--bone) 78%, var(--bedrock))`.
    pub fn pulse_light() -> Rgba {
        mix(palette().bone, 0.78, palette().bedrock)
    }
    /// `conv-pulse`'s `to`: `color-mix(in srgb, var(--bone) 22%, var(--bedrock))`.
    pub fn pulse_dark() -> Rgba {
        mix(palette().bone, 0.22, palette().bedrock)
    }
    /// `conv-arrive`'s `from`: `color-mix(in srgb, var(--bone) 50%, var(--bedrock))`.
    pub fn arrive_from() -> Rgba {
        mix(palette().bone, 0.5, palette().bedrock)
    }
    /// `--waiting`.
    pub fn waiting() -> Rgba {
        palette().waiting
    }
    /// The unread dot's glow at `strength` (1 is
    /// `0 0 6px color-mix(in srgb, var(--waiting) 45%, transparent)`).
    pub fn waiting_glow(strength: f32) -> Vec<BoxShadow> {
        shadow::of(&[layer(
            false,
            0.,
            0.,
            6.,
            0.,
            fade(palette().waiting, 0.45 * strength),
        )])
    }
    /// The search box while its field has focus:
    /// `inset 0 0 0 1px color-mix(in srgb, var(--bone) 22%, transparent)`.
    pub fn find_focus() -> Vec<BoxShadow> {
        shadow::of(&[layer(true, 0., 0., 0., 1., fade(palette().bone, 0.22))])
    }
    /// `.resolution-tag`'s ring:
    /// `inset 0 0 0 1px color-mix(in srgb, currentColor 28%, transparent)`
    /// on `--dim-text`.
    pub fn tag_ring() -> Vec<BoxShadow> {
        shadow::of(&[layer(true, 0., 0., 0., 1., fade(palette().dim_text, 0.28))])
    }
    /// `.cmdk-overlay`: `rgba(0, 0, 0, 0.5)`.
    pub fn palette_scrim() -> Hsla {
        Rgba {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 0.5,
        }
        .into()
    }
}

// settings agent: Settings' and the setup screen's colours and shadows.
/// Settings' own colours and shadows (`style-viewer-routes.md` 1.10, 1.11).
pub mod settings {
    use super::{fade, layer, mix, palette, shadow};
    use gpui_kit::{BoxShadow, Hsla, Rgba};

    /// `.overlay`: `rgba(0, 0, 0, 0.62)`.
    pub fn scrim() -> Hsla {
        Rgba {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 0.62,
        }
        .into()
    }
    /// `.s-section-head h4`: `color-mix(in srgb, var(--bone) 70%, transparent)`.
    pub fn section_head() -> Hsla {
        fade(palette().bone, 0.7).into()
    }
    /// `.s-card`: `color-mix(in srgb, var(--well) 70%, transparent)`.
    pub fn card() -> Hsla {
        fade(palette().well, 0.7).into()
    }
    /// `.s-card`'s ring: `0 0 0 1px var(--line)`.
    pub fn card_ring() -> Vec<BoxShadow> {
        shadow::of(&[layer(false, 0., 0., 0., 1., palette().line)])
    }
    /// A row's top hairline: `color-mix(in srgb, var(--bone) 5%, transparent)`.
    pub fn hairline() -> Hsla {
        fade(palette().bone, 0.05).into()
    }
    /// `.s-row-help`: `color-mix(in srgb, var(--dim-text) 85%, transparent)`.
    pub fn help() -> Hsla {
        fade(palette().dim_text, 0.85).into()
    }
    /// `.setting-flash`'s start: `color-mix(in srgb, var(--bone) 8%, transparent)`.
    pub fn flash() -> Hsla {
        fade(palette().bone, 0.08).into()
    }
    /// `.settings-error`: `1px solid color-mix(in srgb, var(--redstone) 35%, transparent)`.
    pub fn error_border() -> Hsla {
        fade(palette().redstone, 0.35).into()
    }
    /// `.settings-error`'s fill: `rgba(224, 93, 79, 0.08)`.
    pub fn error_fill() -> Hsla {
        Rgba {
            r: 224. / 255.,
            g: 93. / 255.,
            b: 79. / 255.,
            a: 0.08,
        }
        .into()
    }
    /// A focused `.s-input` or `.s-filter`:
    /// `inset 0 0 0 1px color-mix(in srgb, var(--gold) 70%, transparent)`,
    /// with `--glow-gold` for `.s-input`.
    pub fn input_focus(glow: bool) -> Vec<BoxShadow> {
        let mut shadows = shadow::of(&[layer(true, 0., 0., 0., 1., fade(palette().gold, 0.7))]);
        if glow {
            shadows.extend(shadow::glow_gold());
        }
        shadows
    }
    /// `.bind-key`: `inset 0 0 0 1px var(--line), 0 1px 0 rgba(0, 0, 0, 0.35)`.
    pub fn bind_key() -> Vec<BoxShadow> {
        shadow::of(&[
            layer(true, 0., 0., 0., 1., palette().line),
            layer(
                false,
                0.,
                1.,
                0.,
                0.,
                Rgba {
                    r: 0.,
                    g: 0.,
                    b: 0.,
                    a: 0.35,
                },
            ),
        ])
    }
    /// `.bind-key.recording`'s ring at `t` of its breath (20% to 55% bone).
    pub fn bind_listen(t: f32) -> Vec<BoxShadow> {
        shadow::of(&[layer(
            true,
            0.,
            0.,
            0.,
            1.,
            fade(palette().bone, 0.2 + 0.35 * t),
        )])
    }
    /// `.bind-key:hover`'s ring: `--bevel-light`.
    pub fn bind_hover() -> Vec<BoxShadow> {
        shadow::of(&[layer(true, 0., 0., 0., 1., palette().bevel_light)])
    }
    /// `.s-model.dragged`: `0 8px 24px rgba(0, 0, 0, 0.4)`.
    pub fn dragged() -> Vec<BoxShadow> {
        shadow::of(&[layer(
            false,
            0.,
            8.,
            24.,
            0.,
            Rgba {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 0.4,
            },
        )])
    }
    /// `.connect-way` and `.setup-steps`: `inset 0 0 0 1px var(--line)`.
    pub fn inset_line() -> Vec<BoxShadow> {
        shadow::of(&[layer(true, 0., 0., 0., 1., palette().line)])
    }
    /// `.setup-bar`'s track: `color-mix(in srgb, var(--bone) 10%, transparent)`.
    pub fn bar_track() -> Hsla {
        fade(palette().bone, 0.1).into()
    }
    /// The danger button's hover: `color-mix(in srgb, var(--redstone) 86%, #000)`.
    pub fn danger_hover() -> Hsla {
        mix(
            palette().redstone,
            0.86,
            Rgba {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 1.,
            },
        )
        .into()
    }
}

/// Corner radii the stylesheet uses.
pub mod radius {
    /// `--radius-sm`, `.btn`, `.icon-btn`, `.header-action`.
    pub const SM: f32 = 8.;
    /// A chat row (`.conv-item`), a side title.
    pub const ROW: f32 = 7.;
    /// Small row icons (`.row-icon`, `.conv-item .archive`), an error's
    /// dismiss.
    pub const ICON: f32 = 6.;
    /// The tooltip.
    pub const TIP: f32 = 7.;
    /// `.tag-menu`.
    pub const MENU: f32 = 10.;
    /// The composer box.
    pub const COMPOSER: f32 = 12.;
    /// The user's message.
    pub const USER: f32 = 13.;
    /// An error's box.
    pub const ERROR: f32 = 9.;
}

/// The transition and animation easing, `--ease`: `cubic-bezier(0.2, 0.7,
/// 0.3, 1)`.
pub const EASE: (f32, f32, f32, f32) = (0.2, 0.7, 0.3, 1.);

/// `--ease` at `t` (0 to 1): the cubic Bézier's y for the x `t`.
pub fn ease(t: f32) -> f32 {
    let (x1, y1, x2, y2) = EASE;
    // Solve x(s) = t for s by Newton's method, then y(s).
    let bezier = |a: f32, b: f32, s: f32| {
        3. * (1. - s) * (1. - s) * s * a + 3. * (1. - s) * s * s * b + s * s * s
    };
    let derivative = |a: f32, b: f32, s: f32| {
        3. * (1. - s) * (1. - s) * a + 6. * (1. - s) * s * (b - a) + 3. * s * s * (1. - b)
    };
    let t = t.clamp(0., 1.);
    let mut s = t;
    for _ in 0..8 {
        let dx = derivative(x1, x2, s);
        if dx.abs() < 1e-6 {
            break;
        }
        s = (s - (bezier(x1, x2, s) - t) / dx).clamp(0., 1.);
    }
    bezier(y1, y2, s)
}

/// The font families the page's stacks resolve to.
#[derive(Clone, Debug)]
pub struct Fonts {
    /// `--font-body` and `--font-ui` (the same stack).
    pub ui: SharedString,
    /// `--font-data`.
    pub data: SharedString,
}

/// Which stack a text uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// `--font-body`.
    Body,
    /// `--font-ui` (the same stack as the body).
    Ui,
    /// `--font-data`.
    Data,
}

/// A line height: in pixels, or relative to the size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Leading {
    /// The browser's `normal`.
    Normal,
    /// `line-height: <n>px`.
    Px(f32),
    /// `line-height: <n>`.
    Times(f32),
}

/// One kind of text's style (inventory 1.3).
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    /// The stack.
    pub family: Family,
    /// The size in pixels.
    pub size: f32,
    /// The line height.
    pub leading: Leading,
    /// The weight.
    pub weight: f32,
    /// The letter spacing in pixels (not drawn: GPUI has none).
    pub tracking: f32,
    /// The colour.
    pub colour: fn() -> Hsla,
    /// Tabular numbers (`font-variant-numeric: tabular-nums`).
    pub tabular: bool,
}

/// The kinds of text of the page (inventory 1.3), each with its style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKind {
    /// `.page-loading`.
    Loading,
    /// `.brand .wordmark`.
    Wordmark,
    /// `.brand .wordmark em`.
    WordmarkNote,
    /// `.mode-word`.
    ModeWord,
    /// `.conv-item`.
    SidebarRow,
    /// `.conv-snippet`.
    RowSnippet,
    /// `.bench-member`.
    BenchMember,
    /// `.side-title`.
    SideTitle,
    /// `.side-find input`.
    SideSearch,
    /// `.sidebar .meta.warn`.
    SidebarWarning,
    /// `.archived-row`.
    ArchivedRow,
    /// `.archived-row` meta.
    ArchivedMeta,
    /// `.resolution-tag`.
    ResolutionTag,
    /// `.chat-header .title`.
    PaneTitle,
    /// `.chat-header .crumb`.
    Crumb,
    /// `.header-action`.
    HeaderAction,
    /// `.msg`.
    Prose,
    /// `.reasoning-text`.
    Reasoning,
    /// `.log-note`.
    Note,
    /// `.log-step .s`.
    ToolName,
    /// `.log-step .d`.
    ToolDetail,
    /// Tool meta and the diff stat.
    ToolMeta,
    /// `.log-err`.
    ToolError,
    /// `.code`.
    Code,
    /// `.code.term`.
    Terminal,
    /// `.ticker`.
    Ticker,
    /// `.activity .steps`.
    FoldSummary,
    /// `.msg.error`.
    Error,
    /// `.empty-hint .big`.
    EmptyHeading,
    /// `.empty-sub`.
    EmptySub,
    /// `.composer-input`.
    Composer,
    /// `.btn`.
    Button,
    /// `.model-picker-trigger`.
    PickerTrigger,
    /// The effort and resolution triggers.
    SmallTrigger,
    /// `.model-name`.
    ModelName,
    /// `.settings-nav-title`.
    SettingsNavTitle,
    /// `.settings-nav-label`.
    SettingsNavLabel,
    /// `.settings-nav-item .name`.
    SettingsNavItem,
    /// `.pane-head h3`.
    SettingsPaneTitle,
    /// `.s-section-head h4`.
    SettingsSectionHead,
    /// `.s-row-title`.
    SettingsRowTitle,
    /// `.s-row-help`.
    SettingsRowHelp,
    /// `.s-value`.
    SettingsValue,
    /// `.s-tag`.
    Tag,
    /// `.pe-title`.
    PromptTitle,
    /// `.pe-body`.
    PromptBody,
    /// `.cmdk-input input`.
    PaletteInput,
    /// `.setup .big`.
    SetupHeading,
    /// `.crash h1`.
    CrashHeading,
    /// `.crash pre`.
    CrashText,
    /// The tooltip (`[data-tip]::after`).
    Tooltip,
    /// `.tag-menu` items.
    MenuItem,
    // pickers: the composer's pickers and meters.
    /// The limit chip (`.limit-chip`).
    LimitChip,
    /// The model picker's search (`.model-picker-search input`).
    PickerSearch,
    /// The model picker's empty, loading and error lines.
    PickerNote,
    /// A model row's provider while searching (`.model-provider`).
    PickerProvider,
    /// A harness's `›_` badge on the rail.
    HarnessBadge,
    /// The footer's row (`.model-picker-footer`).
    PickerFooter,
    /// No AI connected (`.model-picker-no-ai-text`).
    PickerNoAi,
    /// An effort option (`.effort-picker-menu` label).
    EffortOption,
    /// The resolution popover's head (`.resolution-picker-head`).
    ResolutionHead,
    /// A tick under the slider.
    ResolutionTick,
    /// The help under the slider.
    ResolutionHelp,
    // pane agent: the 3D pane's texts.
    /// `.live-ticks`: 12 px, tabular numbers.
    LiveTicks,
    /// `.bluemap-mismatch`: 13 px 600 `--ash`.
    LiveBanner,
    /// `.bluemap-placeholder`, the cover: 13 px 550 `--ash`.
    LiveCover,
    /// `.mcbench-tag`: 500 12 px `#f5f5f5`.
    LiveTag,
    /// The player HUD (`#mcbench-player`): `12px/1.3 system-ui`, `#d8d3c8`.
    LiveHud,
    /// The player's note: as the HUD, `#ffd7a8`.
    LiveNote,
    /// The pane's own bar's buttons (BlueMap's control bar).
    LiveBar,
    // par-chat (stage four's parity pass)
    /// `.cap-warn`: the header's model warning, 11 px 500 `--err-text`.
    CapWarn,
}

impl TextKind {
    /// The kind's style.
    pub fn style(self) -> TextStyle {
        use Family::*;
        use Leading::*;
        let s = |family, size, leading, weight, tracking, colour: fn() -> Hsla| TextStyle {
            family,
            size,
            leading,
            weight,
            tracking,
            colour,
            tabular: false,
        };
        match self {
            TextKind::Loading => s(Ui, 13., Normal, 400., 0., colour::dim),
            TextKind::Wordmark => s(Body, 14., Times(1.), 650., -0.2, colour::bone),
            TextKind::WordmarkNote => s(Body, 11., Times(1.), 500., 0., colour::ash),
            TextKind::ModeWord => s(Body, 14., Times(1.), 650., -0.2, colour::mode_word),
            TextKind::SidebarRow => s(Body, 13., Normal, 430., 0., colour::ash),
            TextKind::RowSnippet => s(Body, 12., Normal, 400., 0., colour::dim),
            TextKind::BenchMember => s(Body, 12.5, Normal, 430., 0., colour::ash),
            TextKind::SideTitle => s(Ui, 13., Normal, 500., 0., colour::dim),
            TextKind::SideSearch => s(Body, 13., Normal, 400., 0., colour::bone),
            TextKind::SidebarWarning => s(Body, 12., Times(1.5), 400., 0., colour::err),
            TextKind::ArchivedRow => s(Body, 13., Normal, 400., 0., colour::ash),
            TextKind::ArchivedMeta => s(Body, 11.5, Normal, 400., 0., colour::dim),
            TextKind::ResolutionTag => s(Ui, 10.5, Px(16.), 500., 0., colour::dim),
            TextKind::PaneTitle => s(Body, 13.5, Normal, 570., -0.1, colour::bone),
            TextKind::Crumb => s(Body, 13.5, Normal, 500., 0., colour::ash),
            TextKind::HeaderAction => s(Ui, 12.5, Normal, 500., 0., colour::bedrock),
            TextKind::Prose => s(Body, 14., Times(1.62), 400., 0., colour::bone),
            TextKind::Reasoning => s(Body, 12.5, Times(1.6), 400., 0., colour::reason),
            TextKind::Note => s(Body, 13.5, Times(1.62), 400., 0., colour::note),
            TextKind::ToolName => s(Body, 12., Normal, 550., 0., colour::bone_82),
            TextKind::ToolDetail => s(Body, 12., Normal, 400., 0., colour::dim_82),
            TextKind::ToolMeta => s(Data, 10., Normal, 400., 0., colour::dim),
            TextKind::ToolError => s(Data, 11., Times(1.5), 400., 0., colour::err_soft),
            TextKind::Code => s(Data, 11., Times(1.65), 400., 0., colour::dim),
            TextKind::Terminal => s(Data, 11., Times(1.65), 400., 0., colour::term),
            TextKind::Ticker => TextStyle {
                tabular: true,
                ..s(Body, 11., Px(20.), 400., 0., colour::dim)
            },
            TextKind::FoldSummary => s(Body, 12., Normal, 450., 0., colour::ash),
            TextKind::Error => s(Body, 13., Normal, 400., 0., colour::err),
            TextKind::EmptyHeading => s(Body, 20., Times(1.25), 620., -0.4, colour::bone),
            TextKind::EmptySub => s(Body, 13., Normal, 400., 0., colour::dim),
            TextKind::Composer => s(Body, 14., Px(20.), 400., 0., colour::bone),
            TextKind::Button => s(Body, 13., Normal, 500., 0., colour::bone),
            TextKind::PickerTrigger => s(Ui, 12.5, Normal, 450., 0., colour::ash),
            TextKind::SmallTrigger => s(Body, 12., Normal, 400., 0., colour::ash),
            TextKind::ModelName => s(Body, 12.5, Normal, 470., 0.1, colour::bone),
            TextKind::SettingsNavTitle => s(Body, 15., Normal, 600., 0., colour::bone),
            TextKind::SettingsNavLabel => s(Body, 11.5, Normal, 500., 0., colour::dim),
            TextKind::SettingsNavItem => s(Body, 13.5, Times(1.3), 500., 0., colour::bone),
            TextKind::SettingsPaneTitle => s(Body, 17., Normal, 600., -0.17, colour::bone),
            TextKind::SettingsSectionHead => s(Body, 13., Normal, 400., 0., colour::bone),
            TextKind::SettingsRowTitle => s(Body, 13.5, Normal, 500., 0., colour::bone),
            TextKind::SettingsRowHelp => s(Body, 12., Times(1.45), 400., 0., colour::dim),
            TextKind::SettingsValue => s(Body, 13., Normal, 400., 0., colour::ash),
            TextKind::Tag => s(Body, 11., Normal, 500., 0.11, colour::bone),
            TextKind::PromptTitle => s(Body, 24., Times(1.2), 620., -0.6, colour::bone),
            TextKind::PromptBody => s(Body, 14., Times(1.65), 400., 0., colour::bone),
            TextKind::PaletteInput => s(Body, 15., Normal, 400., 0., colour::bone),
            TextKind::SetupHeading => s(Body, 20., Times(1.25), 620., -0.4, colour::bone),
            TextKind::CrashHeading => s(Ui, 18., Normal, 600., 0., colour::bone),
            TextKind::CrashText => s(Ui, 12.5, Normal, 400., 0., colour::err),
            TextKind::Tooltip => s(Ui, 12., Times(1.45), 400., 0., colour::bone),
            TextKind::MenuItem => s(Ui, 12.5, Normal, 400., 0., colour::ash),
            // pickers
            TextKind::LimitChip => s(Ui, 11.5, Normal, 500., 0., colour::star),
            TextKind::PickerSearch => s(Ui, 13., Normal, 400., 0., colour::bone),
            TextKind::PickerNote => s(Ui, 12.5, Normal, 400., 0., colour::ash),
            TextKind::PickerProvider => s(Ui, 11.5, Normal, 400., 0., colour::dim),
            TextKind::HarnessBadge => s(Data, 8., Times(1.), 700., -0.5, colour::dim),
            TextKind::PickerFooter => s(Ui, 12.5, Normal, 400., 0., colour::ash),
            TextKind::PickerNoAi => s(Ui, 13., Normal, 400., 0., colour::ash),
            TextKind::EffortOption => s(Ui, 12.5, Normal, 500., 0., colour::ash),
            TextKind::ResolutionHead => s(Body, 12.5, Normal, 500., 0., colour::ash),
            TextKind::ResolutionTick => s(Ui, 11., Normal, 400., 0., colour::dim),
            TextKind::ResolutionHelp => s(Body, 11.5, Times(1.45), 400., 0., colour::dim),
            // pane agent
            TextKind::LiveTicks => TextStyle {
                tabular: true,
                ..s(Body, 12., Normal, 400., 0., colour::ash)
            },
            TextKind::LiveBanner => s(Body, 13., Normal, 600., 0., colour::ash),
            TextKind::LiveCover => s(Body, 13., Normal, 550., 0., colour::ash),
            TextKind::LiveTag => s(Ui, 12., Normal, 500., 0., colour::live_tag_text),
            TextKind::LiveHud => s(Ui, 12., Times(1.3), 400., 0., colour::live_hud_text),
            TextKind::LiveNote => s(Ui, 12., Times(1.3), 400., 0., colour::live_note_text),
            // BlueMap's bar: 16 px at weight 400 (`.theme-dark`), drawn in
            // the app's sans (BlueMap's Quicksand is not in the app).
            TextKind::LiveBar => s(Ui, 16., Normal, 400., 0., colour::live_bar_text),
            // par-chat: `.cap-warn` (S:945-956).
            TextKind::CapWarn => s(Body, 11., Normal, 500., 0., colour::err),
        }
    }
}

/// Applies a kind of text's family, size, line height, weight and colour.
pub fn text<E: Styled>(element: E, kind: TextKind, fonts: &Fonts) -> E {
    let style = kind.style();
    let family = match style.family {
        Family::Body | Family::Ui => fonts.ui.clone(),
        Family::Data => fonts.data.clone(),
    };
    let element = element
        .font_family(family)
        .text_size(px(style.size))
        .font_weight(FontWeight(style.weight))
        .text_color((style.colour)());
    match style.leading {
        Leading::Normal => element,
        Leading::Px(value) => element.line_height(px(value)),
        Leading::Times(value) => element.line_height(relative(value)),
    }
}

/// The root font size (`:root { font-size: 14px }`), the base of `em`.
pub const ROOT_SIZE: Pixels = px(14.);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dark_tokens_are_the_stylesheets_final_values() {
        assert_eq!(DARK.bedrock, hex(0x0a0a0a));
        assert_eq!(DARK.bone, hex(0xf5f5f5));
        assert!((DARK.line.a - 0.06).abs() < 1e-6);
        assert_eq!(LIGHT_PALETTE.gold, hex(0x18181b));
        assert_eq!(DARK_SHADOWS.control_face.len(), 3);
        assert!(DARK_SHADOWS.control_face[0].inset);
    }

    #[test]
    fn each_theme_has_its_own_palette_and_dark_is_the_default() {
        assert_eq!(palette_for(Mode::Light).bedrock, hex(0xfafafa));
        assert_eq!(palette_for(Mode::Dark).bedrock, hex(0x0a0a0a));
        assert_eq!(mode(), Mode::Dark);
    }

    #[test]
    fn the_ease_curve_starts_and_ends_in_place() {
        assert!(ease(0.).abs() < 1e-4);
        assert!((ease(1.) - 1.).abs() < 1e-4);
        assert!(ease(0.5) > 0.5, "--ease front-loads its motion");
    }
}
