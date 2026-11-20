// A read-back picture to what see_build returns: the picture is
// premultiplied (as a WebGL canvas holds it), the labelled grid is drawn
// over it (draw_grid: as a 2D canvas draws lines, boxes and text) and its
// JPEG holds each pixel's colour as blended, alpha dropped. Here the same in premultiplied f32, the grid's
// lines and boxes covering pixels by their area as the canvas's
// antialiasing does, its text in Liberation Sans Bold (bundled: fonts/,
// SIL Open Font License 1.1) instead of the browser's sans-serif.

use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub at: f64,
    pub major: bool,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub at: f64,
    pub text: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Lines {
    pub edges: Vec<Edge>,
    pub labels: Vec<Label>,
}

// server/shot.js gridFor's grid.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Grid {
    pub columns: Lines,
    pub rows: Lines,
    pub legend: String,
    pub step: f64,
}

// Premultiplied 8-bit RGBA, as the page's 2D canvas holds a picture.
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub px: Vec<u8>,
}

impl Canvas {
    // The drawing buffer as the page's 2D canvas takes it: colour clamped
    // to alpha (a premultiplied colour cannot pass its alpha).
    pub fn from_rgba(mut rgba: Vec<u8>, width: u32, height: u32) -> Canvas {
        for p in rgba.as_chunks_mut::<4>().0 {
            for i in 0..3 {
                p[i] = p[i].min(p[3]);
            }
        }
        Canvas {
            width: width as usize,
            height: height as usize,
            px: rgba,
        }
    }

    // Source-over of `rgba` (straight, 0-255 and alpha 0-1) at a pixel,
    // `cover` of it covered, rounded to 8 bits as the canvas stores it.
    fn over(&mut self, x: usize, y: usize, rgba: [f32; 4], cover: f32) {
        let a = rgba[3] * cover;
        if a <= 0.0 {
            return;
        }
        let at = (y * self.width + x) * 4;
        let p = &mut self.px[at..at + 4];
        for i in 0..3 {
            p[i] = (rgba[i] * a + p[i] as f32 * (1.0 - a))
                .round()
                .clamp(0.0, 255.0) as u8;
        }
        p[3] = (255.0 * a + p[3] as f32 * (1.0 - a))
            .round()
            .clamp(0.0, 255.0) as u8;
    }

    // A rectangle of fractional edges, each pixel covered by its overlap.
    pub fn fill_rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, rgba: [f32; 4]) {
        let (cx0, cx1) = (x0.max(0.0), x1.min(self.width as f64));
        let (cy0, cy1) = (y0.max(0.0), y1.min(self.height as f64));
        if cx0 >= cx1 || cy0 >= cy1 {
            return;
        }
        for y in cy0.floor() as usize..cy1.ceil() as usize {
            let cy = (cy1.min(y as f64 + 1.0) - cy0.max(y as f64)).max(0.0);
            for x in cx0.floor() as usize..cx1.ceil() as usize {
                let cx = (cx1.min(x as f64 + 1.0) - cx0.max(x as f64)).max(0.0);
                self.over(x, y, rgba, (cx * cy) as f32);
            }
        }
    }

    // What the JPEG holds: the premultiplied colour.
    pub fn rgb(&self) -> Vec<u8> {
        self.px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect()
    }
}

// A read-back picture to the RGB its JPEG holds, the grid drawn over it.
pub fn finish(rgba: Vec<u8>, width: u32, height: u32, grid: Option<&Grid>, font: &Font) -> Vec<u8> {
    let mut canvas = Canvas::from_rgba(rgba, width, height);
    if let Some(grid) = grid {
        draw_grid(&mut canvas, grid, font);
    }
    canvas.rgb()
}

pub struct Font {
    font: fontdue::Font,
}

const FONT_BYTES: &[u8] = include_bytes!("../fonts/LiberationSans-Bold.ttf");
const SIZE: f32 = 20.0;

impl Font {
    pub fn new() -> Result<Font, String> {
        fontdue::Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
            .map(|font| Font { font })
            .map_err(|e| format!("The bundled font: {e}"))
    }

    fn glyphs(&self, text: &str) -> Vec<(char, f32)> {
        let mut pen = 0.0;
        let mut out = Vec::new();
        let mut previous: Option<char> = None;
        for c in text.chars() {
            if let Some(p) = previous {
                pen += self.font.horizontal_kern(p, c, SIZE).unwrap_or(0.0);
            }
            out.push((c, pen));
            pen += self.font.metrics(c, SIZE).advance_width;
            previous = Some(c);
        }
        out
    }

    // measureText(text).width.
    pub fn width(&self, text: &str) -> f64 {
        let glyphs = self.glyphs(text);
        glyphs
            .last()
            .map(|&(c, pen)| (pen + self.font.metrics(c, SIZE).advance_width) as f64)
            .unwrap_or(0.0)
    }

    // fillText with textBaseline "middle": the em box's middle at `y`.
    fn fill_text(&self, canvas: &mut Canvas, text: &str, x: f64, y: f64, rgba: [f32; 4]) {
        let line = self
            .font
            .horizontal_line_metrics(SIZE)
            .expect("a horizontal font");
        let baseline = y + ((line.ascent + line.descent) / 2.0) as f64;
        for (c, pen) in self.glyphs(text) {
            let (m, bitmap) = self.font.rasterize(c, SIZE);
            let left = (x + pen as f64).round() as i64 + m.xmin as i64;
            let top = baseline.round() as i64 - (m.ymin as i64 + m.height as i64);
            for row in 0..m.height {
                for col in 0..m.width {
                    let (px, py) = (left + col as i64, top + row as i64);
                    if px >= 0
                        && py >= 0
                        && (px as usize) < canvas.width
                        && (py as usize) < canvas.height
                    {
                        canvas.over(
                            px as usize,
                            py as usize,
                            rgba,
                            bitmap[row * m.width + col] as f32 / 255.0,
                        );
                    }
                }
            }
        }
    }
}

// The labelled grid of a shot (`grid`: lines, label boxes and labels, the
// legend) drawn over the picture.
pub fn draw_grid(canvas: &mut Canvas, grid: &Grid, font: &Font) {
    let (w, h) = (canvas.width as f64, canvas.height as f64);
    let stroke = |major: bool| {
        if major {
            ([0.0, 0.0, 0.0, 0.55], 2.0)
        } else {
            ([0.0, 0.0, 0.0, 0.22], 1.0)
        }
    };
    for e in &grid.columns.edges {
        let (rgba, lw) = stroke(e.major);
        let c = e.at.round() + 0.5;
        canvas.fill_rect(c - lw / 2.0, 0.0, c + lw / 2.0, h, rgba);
    }
    for e in &grid.rows.edges {
        let (rgba, lw) = stroke(e.major);
        let c = e.at.round() + 0.5;
        canvas.fill_rect(0.0, c - lw / 2.0, w, c + lw / 2.0, rgba);
    }
    let label = |canvas: &mut Canvas, text: &str, x: f64, y: f64, center: bool| {
        let tw = font.width(text) + 8.0;
        let left = if center { x - tw / 2.0 } else { x };
        canvas.fill_rect(
            left,
            y - 13.0,
            left + tw,
            y + 13.0,
            [255.0, 255.0, 255.0, 0.85],
        );
        let text_left = if center {
            x - font.width(text) / 2.0
        } else {
            x + 4.0
        };
        font.fill_text(canvas, text, text_left, y, [0.0, 0.0, 0.0, 1.0]);
    };
    for l in &grid.columns.labels {
        if l.at > 20.0 && l.at < w - 20.0 {
            label(canvas, &l.text, l.at, 16.0, true);
        }
    }
    for l in &grid.rows.labels {
        if l.at > 40.0 && l.at < h - 20.0 {
            label(canvas, &l.text, 2.0, l.at, false);
        }
    }
    label(
        canvas,
        &grid.legend,
        w - font.width(&grid.legend) - 20.0,
        h - 18.0,
        false,
    );
}

pub fn jpeg(rgb: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let w = u16::try_from(width).map_err(|_| "A picture is too wide for a JPEG")?;
    let h = u16::try_from(height).map_err(|_| "A picture is too tall for a JPEG")?;
    jpeg_encoder::Encoder::new(&mut out, quality)
        .encode(rgb, w, h, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

// The same RGB as a PNG.
pub fn png(rgb: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

// composeMontage: each JPEG at its cell of a card (black elsewhere, as a
// fresh canvas's JPEG is), the card a JPEG.
pub fn montage(
    tiles: &[&[u8]],
    cells: &[Cell],
    size: [u32; 2],
    quality: u8,
) -> Result<Vec<u8>, String> {
    if tiles.len() != cells.len() {
        return Err(format!(
            "A card of {} cells was given {} pictures",
            cells.len(),
            tiles.len()
        ));
    }
    let [cw, ch] = size;
    let mut card = vec![0u8; (cw * ch * 3) as usize];
    for (i, (bytes, cell)) in tiles.iter().zip(cells).enumerate() {
        let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)
            .map_err(|e| format!("Picture {i} of the card: {e}"))?
            .to_rgb8();
        if image.width() != cell.width || image.height() != cell.height {
            return Err(format!(
                "Picture {i} of the card is {}x{}, its cell {}x{}",
                image.width(),
                image.height(),
                cell.width,
                cell.height
            ));
        }
        if cell.x + cell.width > cw || cell.y + cell.height > ch {
            return Err(format!("Cell {i} lies outside the card"));
        }
        for row in 0..cell.height {
            let from = (row * cell.width * 3) as usize;
            let to = (((cell.y + row) * cw + cell.x) * 3) as usize;
            card[to..to + (cell.width * 3) as usize]
                .copy_from_slice(&image.as_raw()[from..from + (cell.width * 3) as usize]);
        }
    }
    jpeg(&card, cw, ch, quality)
}
