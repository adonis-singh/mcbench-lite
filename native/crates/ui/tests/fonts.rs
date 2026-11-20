//! The font stacks' last entries (`mcbench_ui::view::fonts`): with no font
//! on the system every lookup resolves to the embedded sans, a family the
//! text system cannot load gives way to the embedded one, the embedded
//! families draw the stylesheet's weights, and the system's families are
//! fontconfig's own answers. Linux only (GPUI's Linux text system).
#![cfg(target_os = "linux")]

use std::sync::Arc;

use gpui::{
    Font, FontStyle, FontWeight, PlatformTextSystem as _, RenderGlyphParams, TextSystem, font,
    point, px,
};
use gpui_wgpu::CosmicTextSystem;
use mcbench_ui::view::fonts;

/// A text system with no fonts at all, the app's added.
fn bare() -> TextSystem {
    let text = TextSystem::new(Arc::new(CosmicTextSystem::new_without_system_fonts(
        "IBM Plex Sans",
    )));
    fonts::add(&text);
    text
}

fn family_of(text: &TextSystem, family: &str) -> String {
    let id = text.resolve_font(&font(family.to_string()));
    text.get_font_for_id(id)
        .expect("a font resolved directly has that id")
        .family
        .to_string()
}

#[test]
fn with_no_system_font_every_lookup_resolves_to_the_embedded_sans() {
    let text = bare();
    assert_eq!(family_of(&text, ".SystemUIFont"), fonts::SANS);
    assert_eq!(family_of(&text, "FreeSans"), fonts::SANS);
    assert_eq!(family_of(&text, fonts::MONO), fonts::MONO);
}

#[test]
fn each_stack_ends_in_its_embedded_family() {
    let text = bare();
    assert_eq!(fonts::first(&text, None, fonts::SANS), fonts::SANS);
    assert_eq!(
        fonts::first(&text, Some("FreeSans".into()), fonts::SANS),
        fonts::SANS,
        "a family the text system does not have gives way"
    );
    assert_eq!(fonts::first(&text, None, fonts::MONO), fonts::MONO);
    assert_eq!(
        fonts::first(&text, Some(fonts::MONO.into()), fonts::SANS),
        fonts::MONO,
        "a family the text system loads is the family"
    );
}

fn ink(system: &CosmicTextSystem, family: &str, weight: f32) -> u64 {
    let font = Font {
        family: family.to_string().into(),
        features: Default::default(),
        fallbacks: None,
        weight: FontWeight(weight),
        style: FontStyle::Normal,
    };
    let font_id = system.font_id(&font).expect("the family loads");
    let glyph_id = system.glyph_for_char(font_id, 'H').expect("an H");
    let params = RenderGlyphParams {
        font_id,
        glyph_id,
        font_size: px(48.),
        subpixel_variant: point(0, 0),
        scale_factor: 1.,
        is_emoji: false,
        subpixel_rendering: false,
        dilation: 0,
    };
    let bounds = system.glyph_raster_bounds(&params).expect("raster bounds");
    let (_, bytes) = system.rasterize_glyph(&params, bounds).expect("a picture");
    bytes.iter().map(|b| u64::from(*b)).sum()
}

#[test]
fn the_embedded_families_draw_the_stylesheets_weights() {
    let system = CosmicTextSystem::new_without_system_fonts("IBM Plex Sans");
    system
        .add_fonts(vec![fonts::SANS_FILE.into(), fonts::MONO_FILE.into()])
        .expect("the embedded fonts load");
    let mut names = system.all_font_names();
    names.sort();
    assert_eq!(names, [fonts::SANS, fonts::MONO]);
    for family in [fonts::SANS, fonts::MONO] {
        let weights = [400., 430., 520., 650.].map(|w| ink(&system, family, w));
        eprintln!("ink of H in {family} at 400, 430, 520, 650: {weights:?}");
        assert!(
            weights.windows(2).all(|pair| pair[1] > pair[0]),
            "each heavier weight draws heavier in {family}: {weights:?}"
        );
    }
}

/// `fc-match`'s first family for `pattern`, as the app found it before it
/// matched in the process.
fn fc_match(pattern: &str) -> String {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{family}", pattern])
        .output()
        .expect("fc-match runs (the test compares with it)");
    assert!(out.status.success(), "fc-match {pattern}");
    String::from_utf8(out.stdout)
        .expect("UTF-8")
        .split(',')
        .next()
        .expect("a family")
        .trim()
        .to_string()
}

#[test]
fn the_system_families_are_fontconfigs_answers() {
    let library = fonts::fontconfig::Library::open().expect("libfontconfig on the test machine");
    for pattern in ["system-ui", "monospace"] {
        let family = library.family(pattern).expect("a font matches");
        eprintln!("{pattern}: {family}");
        assert_eq!(family, fc_match(pattern), "{pattern}");
    }
}
