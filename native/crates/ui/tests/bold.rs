//! Bold text on Linux, as pictures: GPUI's Linux text system (cosmic-text)
//! drew every weight of a variable font at its default weight, because
//! font-config lists a variable font as one face and every face was loaded
//! at weight 400 (`native/vendor/README.md`, patch 13). This test loads a
//! variable font into the text system, draws "H" at weights 400, 620 and
//! 700, and checks that each heavier glyph has more ink than the lighter;
//! then draws 400 again after 700 and checks it is the 400 picture (the
//! scale context once kept the last glyph's weight).
//!
//! The font is `MCBENCH_VARIABLE_FONT`, or Adwaita Sans (GNOME's variable
//! UI font, the face GPUI falls back to on adipc). Linux only.
#![cfg(target_os = "linux")]

use gpui::{Font, FontStyle, FontWeight, PlatformTextSystem as _, RenderGlyphParams, point, px};
use gpui_wgpu::CosmicTextSystem;

fn ink(system: &CosmicTextSystem, family: &str, weight: f32) -> (u64, Vec<u8>) {
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
    (bytes.iter().map(|b| u64::from(*b)).sum(), bytes)
}

#[test]
fn heavier_weights_draw_heavier_glyphs() {
    let path = std::env::var("MCBENCH_VARIABLE_FONT")
        .unwrap_or_else(|_| "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf".to_string());
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        panic!("the variable font {path} (set MCBENCH_VARIABLE_FONT to another): {e}")
    });
    let system = CosmicTextSystem::new_without_system_fonts("Adwaita Sans");
    system
        .add_fonts(vec![bytes.into()])
        .expect("the font loads");
    let family = system
        .all_font_names()
        .into_iter()
        .next()
        .expect("a family");
    let (regular, regular_picture) = ink(&system, &family, 400.);
    let (semibold, _) = ink(&system, &family, 620.);
    let (bold, _) = ink(&system, &family, 700.);
    eprintln!("ink of H in {family}: 400 {regular}, 620 {semibold}, 700 {bold}");
    assert!(
        semibold as f64 > regular as f64 * 1.15,
        "620 draws heavier than 400"
    );
    assert!(bold > semibold, "700 draws heavier than 620");
    let (again, again_picture) = ink(&system, &family, 400.);
    assert_eq!(
        (again, again_picture),
        (regular, regular_picture),
        "400 after 700 is 400"
    );
}
