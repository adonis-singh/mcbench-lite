//! BlueMap 5.22's control bar as the pane draws it (pane agent): its icons,
//! character for character from BlueMap's own sources, and the mcbench
//! panel's from `web/bluemap-bridge.js`, each drawn as a picture in the
//! colour BlueMap gives it.
//!
//! BlueMap is MIT-licensed (Copyright (c) Blue (Lukas Rieger)
//! <https://bluecolored.de>). The sources are the baked web app's source map
//! of the app's pinned version (`server/bluemap.js` `VERSION = '5.22'`,
//! `web/assets/index-BxD3mV_W.js.map`): each path names its file.
//!
//! Colours: BlueMap's dark theme (`.theme-dark` in the baked
//! `index-BgiqB2rB.css`): `--theme-bg` `#181818`, `--theme-bg-hover`
//! `#222`, `--theme-bg-light` `#444`, `--theme-fg` `#fff`,
//! `--theme-fg-light` `#aaa`, `--theme-moon-day` `#ff0`,
//! `--theme-stars-day` `#fff` (`theme::colour::live_bar*`).

/// An icon of the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BarIcon {
    /// `src/components/ControlBar/DayNightSwitch.vue`.
    DayNight,
    /// `src/components/ControlBar/ControlsSwitch.vue`, the perspective view.
    Perspective,
    /// The same file, the flat view.
    Flat,
    /// The same file, free flight (the page retitles it player mode).
    Player,
    /// `web/bluemap-bridge.js` `EYE_GLYPH` (spectator).
    Spectator,
    /// `src/components/ControlBar/ControlBar.vue`, reset camera.
    Reset,
    /// `src/components/Controls/ZoomButtons.vue`, zoom in.
    ZoomIn,
    /// The same file, zoom out.
    ZoomOut,
    /// `web/bluemap-bridge.js`, the select button.
    Select,
    /// `web/bluemap-bridge.js`, the download (.schem) link.
    Download,
    /// `web/bluemap-bridge.js`, the world export.
    Export,
    /// `web/bluemap-bridge.js` `EXPAND_GLYPH` (fill the window).
    Expand,
    /// `web/bluemap-bridge.js` `COLLAPSE_GLYPH` (back beside the chat).
    Collapse,
}

impl BarIcon {
    /// The drawing inside a 30 by 30 view box.
    fn body(self) -> &'static str {
        match self {
            BarIcon::DayNight => {
                r#"<path fill="{moon}" d="M17.011,19.722c-3.778-1.613-5.533-5.982-3.921-9.76c0.576-1.348,1.505-2.432,2.631-3.204 c-3.418-0.243-6.765,1.664-8.186,4.992c-1.792,4.197,0.159,9.053,4.356,10.844c3.504,1.496,7.462,0.377,9.717-2.476 C20.123,20.465,18.521,20.365,17.011,19.722z"/><circle fill="{stars}" cx="5.123" cy="7.64" r="1.196"/><circle fill="{stars}" cx="23.178" cy="5.249" r="1.195"/><circle fill="{stars}" cx="20.412" cy="13.805" r="1.195"/><circle fill="{stars}" cx="25.878" cy="23.654" r="1.195"/>"#
            }
            BarIcon::Perspective => {
                r#"<path d="M19.475,10.574c-0.166-0.021-0.337-0.036-0.51-0.045c-0.174-0.009-0.35-0.013-0.525-0.011 c-0.176,0.002-0.353,0.01-0.526,0.024c-0.175,0.015-0.347,0.036-0.515,0.063l-13.39,2.189 c-0.372,0.061-0.7,0.146-0.975,0.247c-0.276,0.102-0.5,0.221-0.66,0.349c-0.161,0.129-0.259,0.268-0.282,0.408 c-0.024,0.141,0.028,0.285,0.165,0.421l5.431,5.511c0.086,0.087,0.191,0.167,0.314,0.241s0.263,0.142,0.417,0.202 c0.155,0.062,0.323,0.115,0.502,0.162c0.18,0.046,0.371,0.085,0.569,0.116s0.405,0.054,0.616,0.068 c0.211,0.015,0.427,0.021,0.645,0.017c0.217-0.003,0.436-0.016,0.652-0.037c0.217-0.022,0.431-0.054,0.641-0.095L27.12,17.43 c0.371-0.073,0.679-0.175,0.917-0.296c0.236-0.12,0.404-0.259,0.497-0.407c0.093-0.147,0.111-0.305,0.052-0.461 c-0.059-0.156-0.195-0.313-0.415-0.46l-7.089-4.742c-0.089-0.06-0.192-0.115-0.308-0.166 c-0.116-0.051-0.243-0.097-0.381-0.138c-0.137-0.041-0.283-0.078-0.438-0.108C19.803,10.621,19.641,10.595,19.475,10.574"/>"#
            }
            BarIcon::Flat => {
                r#"<path d="M22.371,4.158c1.65,0,3,1.35,3,3v15.684c0,1.65-1.35,3-3,3H7.629c-1.65,0-3-1.35-3-3V7.158c0-1.65,1.35-3,3-3H22.371z"/>"#
            }
            BarIcon::Player => {
                r#"<path d="M21.927,11.253c-0.256-0.487-0.915-0.885-1.465-0.885h-2.004c-0.55,0-0.726-0.356-0.39-0.792c0,0,0.698-0.905,0.698-2.041 c0-2.08-1.687-3.767-3.767-3.767s-3.767,1.687-3.767,3.767c0,1.136,0.698,2.041,0.698,2.041c0.336,0.436,0.161,0.794-0.389,0.797 l-2.005,0.01c-0.55,0.002-1.21,0.403-1.467,0.889l-3.656,6.924c-0.257,0.487-0.088,1.128,0.375,1.425l1.824,1.171 c0.462,0.297,1.116,0.184,1.451-0.253l0.839-1.092c0.335-0.437,0.662-0.346,0.726,0.2l0.637,5.415 c0.064,0.546,0.567,0.993,1.117,0.993h7.234c0.55,0,1.053-0.447,1.117-0.993l0.635-5.401c0.064-0.546,0.392-0.637,0.727-0.2 l0.828,1.078c0.335,0.437,0.988,0.55,1.451,0.253l1.823-1.171c0.463-0.297,0.633-0.938,0.377-1.425L21.927,11.253z"/>"#
            }
            BarIcon::Spectator => {
                r#"<path d="M15 8.5C8.6 8.5 4 15 4 15s4.6 6.5 11 6.5S26 15 26 15s-4.6-6.5-11-6.5zm0 11a4.5 4.5 0 1 1 0-9 4.5 4.5 0 0 1 0 9zm0-7a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5z"/>"#
            }
            BarIcon::Reset => {
                r#"<rect x="7.085" y="4.341" transform="matrix(0.9774 0.2116 -0.2116 0.9774 3.2046 -1.394)" width="2.063" height="19.875"/><path d="M12.528,5.088c0,0,3.416-0.382,4.479-0.031c1.005,0.332,2.375,2.219,3.382,2.545c1.096,0.354,4.607-0.089,4.607-0.089 l-2.738,8.488c0,0-3.285,0.641-4.344,0.381c-1.049-0.257-2.607-2.015-3.642-2.324c-0.881-0.264-3.678-0.052-3.678-0.052 L12.528,5.088z"/>"#
            }
            BarIcon::ZoomIn => {
                r#"<path d="M22.471,12.95H17.05V7.527c0-1.297-0.917-2.348-2.05-2.348c-1.132,0-2.05,1.051-2.05,2.348v5.423H7.527 c-1.297,0-2.348,0.917-2.348,2.05c0,1.132,1.051,2.05,2.348,2.05h5.423v5.421c0,1.299,0.918,2.351,2.05,2.351 c1.133,0,2.05-1.052,2.05-2.351V17.05h5.421c1.299,0,2.351-0.918,2.351-2.05C24.821,13.867,23.77,12.95,22.471,12.95z"/>"#
            }
            BarIcon::ZoomOut => {
                r#"<path d="M24.821,15c0,1.132-1.052,2.05-2.351,2.05H7.527c-1.297,0-2.348-0.918-2.348-2.05l0,0c0-1.133,1.051-2.05,2.348-2.05 h14.944C23.77,12.95,24.821,13.867,24.821,15L24.821,15z"/>"#
            }
            BarIcon::Select => {
                r#"<path d="M4.5 11V7A2.5 2.5 0 0 1 7 4.5h4v3H7.5V11zM19 4.5h4A2.5 2.5 0 0 1 25.5 7v4h-3V7.5H19zM25.5 19v4a2.5 2.5 0 0 1-2.5 2.5h-4v-3h3.5V19zM11 25.5H7A2.5 2.5 0 0 1 4.5 23v-4h3v3.5H11zM12.5 11h5a1.5 1.5 0 0 1 1.5 1.5v5a1.5 1.5 0 0 1-1.5 1.5h-5a1.5 1.5 0 0 1-1.5-1.5v-5a1.5 1.5 0 0 1 1.5-1.5z"/>"#
            }
            BarIcon::Download => {
                r#"<path d="M13.5 4.5h3v10.4l3.4-3.4 2.1 2.1-7 7-7-7 2.1-2.1 3.4 3.4zM5 20h3v2.5h14V20h3v4a1.5 1.5 0 0 1-1.5 1.5h-17A1.5 1.5 0 0 1 5 24z"/>"#
            }
            BarIcon::Export => {
                r#"<path d="M15 3.5l9.5 5.2v10.6L15 24.5l-9.5-5.2V8.7zm0 2.3L8.2 9.6 15 13.4l6.8-3.8zm-7.3 5.5v6.8l6.3 3.5v-6.8zm14.6 0l-6.3 3.5v6.8l6.3-3.5z"/>"#
            }
            BarIcon::Expand => {
                r#"<path d="M17 5h8v8l-3-3-4 4-2-2 4-4zM13 25H5v-8l3 3 4-4 2 2-4 4z"/>"#
            }
            BarIcon::Collapse => {
                r#"<path d="M25 7l-4 4 3 3h-8V6l3 3 4-4zM5 23l4-4-3-3h8v8l-3-3-4 4z"/>"#
            }
        }
    }
}

impl BarIcon {
    /// Every icon.
    pub const ALL: [BarIcon; 13] = [
        BarIcon::DayNight,
        BarIcon::Perspective,
        BarIcon::Flat,
        BarIcon::Player,
        BarIcon::Spectator,
        BarIcon::Reset,
        BarIcon::ZoomIn,
        BarIcon::ZoomOut,
        BarIcon::Select,
        BarIcon::Download,
        BarIcon::Export,
        BarIcon::Expand,
        BarIcon::Collapse,
    ];

    /// Its name in the asset paths.
    pub fn name(self) -> &'static str {
        match self {
            BarIcon::DayNight => "day-night",
            BarIcon::Perspective => "perspective",
            BarIcon::Flat => "flat",
            BarIcon::Player => "player",
            BarIcon::Spectator => "spectator",
            BarIcon::Reset => "reset",
            BarIcon::ZoomIn => "zoom-in",
            BarIcon::ZoomOut => "zoom-out",
            BarIcon::Select => "select",
            BarIcon::Download => "download",
            BarIcon::Export => "export",
            BarIcon::Expand => "expand",
            BarIcon::Collapse => "collapse",
        }
    }
}

/// The asset paths' prefix ([`asset`]).
pub const PREFIX: &str = "mcbench/bluemap/";

const NORTH: &str = r#"<path d="M14.792,1.04c0.114-0.354,0.299-0.354,0.412,0l4.089,12.729c0.114,0.353-0.097,0.642-0.468,0.642 l-7.651,0.001c-0.371,0-0.581-0.288-0.468-0.642L14.792,1.04z"/>"#;
const SOUTH: &str = r#"<path d="M10.707,16.23c-0.114-0.353,0.097-0.642,0.468-0.642l7.651-0.001c0.371,0,0.581,0.289,0.468,0.642 l-4.086,12.73c-0.113,0.353-0.299,0.353-0.412,0L10.707,16.23z"/>"#;

fn wrap(body: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 30 30" fill="black">{body}</svg>"#
    )
}

/// The SVG of an asset path under [`PREFIX`]: an icon's drawing, or one
/// part of a two-coloured one (`day-night-moon`, `day-night-stars`,
/// `compass-north`, `compass-south`), each drawn as a mask in its colour.
pub fn asset(name: &str) -> Option<String> {
    let name = name.strip_suffix(".svg")?;
    let day_night = BarIcon::DayNight.body();
    let (moon, stars) = day_night.split_at(day_night.find("<circle").expect("the stars"));
    Some(match name {
        "day-night-moon" => wrap(&moon.replace(r#"fill="{moon}" "#, "")),
        "day-night-stars" => wrap(&stars.replace(r#"fill="{stars}" "#, "")),
        "compass-north" => wrap(NORTH),
        "compass-south" => wrap(SOUTH),
        other => wrap(
            &BarIcon::ALL
                .into_iter()
                .find(|i| i.name() == other)?
                .body()
                .replace(r#"fill="{moon}" "#, "")
                .replace(r#"fill="{stars}" "#, ""),
        ),
    })
}

/// A part of the bar drawn as a mask in `colour` (`size` px, turned by
/// `turn` radians about its centre).
pub fn mask(part: &str, colour: gpui_kit::Hsla, size: f32, turn: f32) -> gpui_kit::Svg {
    use gpui_kit::{Styled as _, Transformation, px, radians, svg};
    svg()
        .path(format!("{PREFIX}{part}.svg"))
        .size(px(size))
        .text_color(colour)
        .with_transformation(Transformation::rotate(radians(turn)))
}

/// BlueMap's narrow layout (`$mobile-break`, `@media (max-width:
/// 575.98px)`): the bar spans the pane with no margin and hides its
/// `thin-hide` parts (Day/Night, the view switch, Reset).
pub const NARROW: f32 = 575.98;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_is_an_svg_of_bluemaps_view_box() {
        let mut names: Vec<&str> = BarIcon::ALL.iter().map(|i| i.name()).collect();
        names.extend([
            "day-night-moon",
            "day-night-stars",
            "compass-north",
            "compass-south",
        ]);
        for name in names {
            let s = asset(&format!("{name}.svg")).unwrap_or_else(|| panic!("no {name}"));
            assert!(s.starts_with("<svg") && s.contains(r#"viewBox="0 0 30 30""#));
            assert!(!s.contains('{'), "{name} left a placeholder");
        }
        assert!(asset("nothing.svg").is_none());
    }
}
