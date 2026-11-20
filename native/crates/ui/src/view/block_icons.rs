//! The blocks' icons, today's page's `/api/blocks/<id>/icon.png`
//! (`blockIconUrl`, web/app.js:3062): the core's [`Core::block_icon`] is
//! asked once per name for the run, on the background executor, which also
//! decodes the PNG; a paint draws only pictures already made.
//!
//! | place | page | here |
//! |---|---|---|
//! | a selection chip, in the composer and on a sent message | `.selection-icon` (web/app.js:2544): the icon at 20 px, smoothed (`image-rendering: auto`), or an empty square | [`chip_square`] |
//! | a draft selection's marker over the 3D pane | `.mcbench-icon` (web/bluemap-bridge.js:71-72, 455-458): 20 px on a white ring, pixelated, or a dark square with a white inset ring | `world::Pane` draws [`Icon::marker`] |
//!
//! Any `Err` draws the empty square, as the page's broken image does
//! (its `alt` is empty), and so does empty space (no block).

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::*;

use crate::Core;
use crate::view::theme::colour;

/// The pane marker's picture is the 16 px icon made this many times larger
/// texel by texel, so GPUI's smoothing keeps its texels square
/// (`image-rendering: pixelated`).
const MARKER_SCALE: u32 = 5;

/// A block's icon, made for the two places that draw it.
#[derive(Clone)]
pub struct Icon {
    /// As decoded, for the chips (smoothed).
    pub chip: Arc<RenderImage>,
    /// Texel by texel larger, for the pane's markers (pixelated).
    pub marker: Arc<RenderImage>,
}

/// What the core answered for one name.
#[derive(Clone)]
pub enum Known {
    /// Asked; the answer is on its way.
    Asking,
    /// The icon.
    Shown(Icon),
    /// The core's sentence (or the decoder's).
    Refused(String),
}

/// The run's icons by name (`oak_planks`).
pub struct BlockIcons {
    core: Arc<dyn Core>,
    known: HashMap<String, Known>,
}

impl Global for BlockIcons {}

/// Puts the core the icons are asked of in place (the shell does, as it
/// opens).
pub fn install(core: Arc<dyn Core>, cx: &mut App) {
    cx.set_global(BlockIcons {
        core,
        known: HashMap::new(),
    });
}

/// The name `block` ("minecraft:oak_planks") goes by in the address:
/// `minecraft:` taken off, as `blockIconUrl` does.
pub fn name_of(block: &str) -> &str {
    block
        .strip_prefix("minecraft:")
        .expect("a selection's block is a minecraft: id")
}

/// `block`'s icon when it is in, `None` for empty space, while it is
/// asked, or when the core said why not. The first sight of a name asks
/// the core; the window is drawn again when the answer is in.
pub fn icon(block: Option<&str>, cx: &mut App) -> Option<Icon> {
    let name = name_of(block?).to_string();
    let icons = cx.global_mut::<BlockIcons>();
    match icons.known.get(&name) {
        Some(Known::Shown(icon)) => return Some(icon.clone()),
        Some(_) => return None,
        None => {}
    }
    icons.known.insert(name.clone(), Known::Asking);
    let core = icons.core.clone();
    let asked = name.clone();
    let answer = cx
        .background_executor()
        .spawn(async move { core.block_icon(&asked).and_then(|png| decode(&png)) });
    cx.spawn(async move |cx| {
        let known = match answer.await {
            Ok(icon) => Known::Shown(icon),
            Err(why) => Known::Refused(why),
        };
        cx.update(|cx| {
            cx.global_mut::<BlockIcons>().known.insert(name, known);
            cx.refresh_windows();
        });
    })
    .detach();
    None
}

/// What the core answered for `name` so far (`None`: never asked).
pub fn known(name: &str, cx: &App) -> Option<Known> {
    cx.global::<BlockIcons>().known.get(name).cloned()
}

/// The PNG as the two pictures, in GPUI's BGRA.
fn decode(png: &[u8]) -> Result<Icon, String> {
    let rgba = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|e| format!("The block's icon could not be read: {e}"))?
        .into_rgba8();
    let (w, h) = rgba.dimensions();
    let large = image::imageops::resize(
        &rgba,
        w * MARKER_SCALE,
        h * MARKER_SCALE,
        image::imageops::FilterType::Nearest,
    );
    let picture = |mut pixels: image::RgbaImage| {
        for pixel in pixels.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]))
    };
    Ok(Icon {
        chip: picture(rgba),
        marker: picture(large),
    })
}

/// `.selection-icon`: a chip's 20 px square, rounded 4, the icon or an
/// empty square outlined in `--dim`. `key` names it for the tests
/// (`<key>-icon` or `<key>-empty`).
pub fn chip_square(key: &str, block: Option<&str>, cx: &mut App) -> AnyElement {
    let square = div().flex_none().size(px(20.)).rounded(px(4.));
    match icon(block, cx) {
        Some(icon) => square
            .debug_selector(|| format!("{key}-icon"))
            .overflow_hidden()
            .child(img(icon.chip).size_full().rounded(px(4.)))
            .into_any_element(),
        None => square
            .debug_selector(|| format!("{key}-empty"))
            .border(px(1.5))
            .border_color(colour::dim())
            .into_any_element(),
    }
}
