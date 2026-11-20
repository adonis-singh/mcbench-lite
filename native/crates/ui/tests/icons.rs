//! The icons and logos as pictures: every icon of the page's `Icon` and
//! every vendor logo is rendered by GPUI's SVG renderer (resvg, as the
//! window draws them), each must leave ink, the gradient logos must carry
//! their brand's colours, and the whole set is written as one contact sheet
//! (`MCBENCH_PICTURES=<folder>` writes `icons.png` there). No window.

use std::sync::Arc;

use gpui::SvgRenderer;
use mcbench_ui::view::icons::{self, Assets, Brand, Icon, Logo};

/// One picture: its width, height and BGRA pixels.
struct Picture(u32, u32, Vec<u8>);

fn render(renderer: &SvgRenderer, svg: &[u8], scale: f32) -> Picture {
    let image = renderer
        .render_single_frame(svg, scale)
        .expect("the SVG renders");
    let size = image.size(0);
    Picture(
        size.width.0 as u32,
        size.height.0 as u32,
        image.as_bytes(0).expect("a frame").to_vec(),
    )
}

fn ink(picture: &Picture) -> u64 {
    picture.2.chunks(4).map(|p| u64::from(p[3])).sum()
}

#[test]
fn every_icon_and_logo_draws() {
    let renderer = SvgRenderer::new(Arc::new(Assets));
    let mut sheet = Vec::new();
    assert_eq!(Icon::ALL.len(), 47, "the page's Icon names 47 drawings");
    for icon in Icon::ALL {
        let picture = render(&renderer, icons::icon_svg(icon).as_bytes(), 1.);
        assert!(
            picture.0 >= 24 && picture.0 == picture.1,
            "{} is square, {}x{}",
            icon.name(),
            picture.0,
            picture.1
        );
        assert!(ink(&picture) > 255 * 6, "{} leaves ink", icon.name());
        sheet.push(picture);
    }
    for logo in Logo::ALL {
        let bytes: Vec<u8> = match icons::brand(logo) {
            Brand::Gradient(angle, stops) => icons::gradient_svg(logo, angle, stops).into_bytes(),
            _ => gpui::AssetSource::load(&Assets, &format!("mcbench/logo/{}.svg", logo.name()))
                .expect("loads")
                .expect("a logo file")
                .into_owned(),
        };
        let picture = render(&renderer, &bytes, 1.);
        assert!(ink(&picture) > 255 * 6, "{} leaves ink", logo.name());
        if let Brand::Gradient(..) = icons::brand(logo) {
            // Coloured, not grey: some pixel's channels differ.
            let coloured = picture
                .2
                .chunks(4)
                .any(|p| p[3] > 200 && (p[0] as i32 - p[2] as i32).abs() > 40);
            assert!(coloured, "{} carries its gradient", logo.name());
        }
        sheet.push(picture);
    }
    let wire = render(&renderer, icons::MARK_WIRE.as_bytes(), 1.);
    let block = render(&renderer, icons::MARK_BLOCK.as_bytes(), 1.);
    assert!(ink(&wire) > 0 && ink(&block) > 0, "the app's mark draws");
    sheet.push(wire);
    sheet.push(block);
    if let Ok(folder) = std::env::var("MCBENCH_PICTURES") {
        let path = std::path::Path::new(&folder).join("icons.png");
        std::fs::write(&path, contact_sheet(&sheet)).expect("the contact sheet is written");
        eprintln!("wrote {}", path.display());
    }
}

/// The pictures on a light grey grid, 12 to a row, 72 px a cell (each
/// scaled to fit), as a PNG.
fn contact_sheet(pictures: &[Picture]) -> Vec<u8> {
    let cell = 72u32;
    let columns = 12u32;
    let rows = (pictures.len() as u32).div_ceil(columns);
    let (width, height) = (cell * columns, cell * rows);
    let mut rgb = vec![235u8; (width * height * 3) as usize];
    for (index, picture) in pictures.iter().enumerate() {
        let (cx, cy) = (
            (index as u32 % columns) * cell,
            (index as u32 / columns) * cell,
        );
        let scale = (picture.0.max(picture.1) as f32 / 48.).max(1. / 2.);
        for y in 0..48u32 {
            for x in 0..48u32 {
                let (sx, sy) = ((x as f32 * scale) as u32, (y as f32 * scale) as u32);
                if sx >= picture.0 || sy >= picture.1 {
                    continue;
                }
                let p = &picture.2[((sy * picture.0 + sx) * 4) as usize..][..4];
                let alpha = p[3] as f32 / 255.;
                let at = (((cy + 12 + y) * width + cx + 12 + x) * 3) as usize;
                // BGRA, premultiplied; an alpha-only icon draws black.
                for (c, v) in [p[2], p[1], p[0]].iter().enumerate() {
                    let base = rgb[at + c] as f32;
                    rgb[at + c] = (base * (1. - alpha) + *v as f32).min(255.) as u8;
                }
            }
        }
    }
    png(width, height, &rgb)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// An RGB PNG with stored (uncompressed) deflate blocks.
fn png(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let mut raw = Vec::new();
    for row in rgb.chunks((width * 3) as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &header);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}
