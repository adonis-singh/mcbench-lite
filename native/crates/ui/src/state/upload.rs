//! A file as a message carries it (`web/app.js` `checkAttachmentLimits`,
//! `readUpload`, `web/upload-image.mjs` `paintUpload`): the limits a
//! draft's files keep, and a file read for sending, a picture redrawn as the
//! model gets it. Reading and redrawing run off the interface thread
//! (`view::chat::attachments`).

use std::path::Path;
use std::sync::Arc;

use crate::OutgoingFile;

/// The longest edge of an attached picture after it is redrawn.
pub const MAX_IMAGE_EDGE: u32 = 1568;

// The attachment limits are the core's (`mcbench_agent`): the core checks a
// message's files again and its limits are the rule; these refuse a pick
// before the send, with the page's texts built from them. A send the core
// refuses comes back as `Update::SendRefused` with its sentence, and the
// draft and its files go back.
pub use mcbench_agent::{MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS, MAX_ATTACHMENTS_BYTES};

/// Bytes as the limits' texts write them (MiB, written `MB` as the page).
fn megabytes(bytes: u64) -> u64 {
    bytes / (1024 * 1024)
}
/// The redrawn picture's JPEG quality (the page's `0.88`).
pub const JPEG_QUALITY: u8 = 88;

/// Whether `added` may join the draft's files `current` (name and size,
/// files still being read counted): the page's texts when not.
pub fn check_attachment_limits(
    current: &[(String, u64)],
    added: &[(String, u64)],
) -> Result<(), String> {
    if current.len() + added.len() > MAX_ATTACHMENTS {
        return Err(format!(
            "A message supports at most {MAX_ATTACHMENTS} attachments"
        ));
    }
    let total: u64 = current.iter().chain(added).map(|(_, size)| size).sum();
    if total > MAX_ATTACHMENTS_BYTES {
        return Err(format!(
            "Attachments exceed {} MB total",
            megabytes(MAX_ATTACHMENTS_BYTES)
        ));
    }
    Ok(())
}

// par-chat (stage four's parity pass): the type a picked file goes out
// with. The page sends `file.type || 'application/octet-stream'`; Chromium
// types a user's file by its name's last extension, ASCII case-insensitive
// (`blink::File` with `kAllContentTypes`, `net::GetMimeTypeFromExtension`):
// its `kPrimaryMappings` first, the platform's registry, then
// `kSecondaryMappings`, the first entry that lists the extension winning.
// The table below is those two lists of Chromium 156.0.8078.4
// (`net/base/mime_util.cc`, the version the redraw was measured against)
// in that order; the platform's registry between them is not read (a
// differences row). The type reaches the model in each file's
// `[Uploaded file: <name> | <type> | …]` line (`server/agent.js`), decides
// the redraw and the image input (`image/jpeg`, `png`, `gif`, `webp`) and
// the stored extension (`server/store.js` `saveAttachments`).
/// Chromium's `kPrimaryMappings` then `kSecondaryMappings`.
const CHROMIUM_TYPES: &[(&str, &str)] = &[
    // kPrimaryMappings.
    ("video/webm", "webm"),
    ("audio/mpeg", "mp3"),
    ("application/wasm", "wasm"),
    ("application/x-chrome-extension", "crx"),
    ("application/xhtml+xml", "xhtml,xht,xhtm"),
    ("audio/flac", "flac"),
    ("audio/matroska", "mka"),
    ("audio/mp3", "mp3"),
    ("audio/ogg", "ogg,oga,opus"),
    ("audio/wav", "wav"),
    ("audio/webm", "webm"),
    ("audio/x-m4a", "m4a"),
    ("image/avif", "avif"),
    ("image/gif", "gif"),
    ("image/jpeg", "jpeg,jpg,jpe"),
    ("image/jxl", "jxl"),
    ("image/png", "png"),
    ("image/apng", "png,apng"),
    ("image/svg+xml", "svg,svgz"),
    ("image/webp", "webp"),
    ("multipart/related", "mht,mhtml"),
    ("text/css", "css"),
    ("text/html", "html,htm,shtml,shtm"),
    ("text/javascript", "js,mjs"),
    ("text/xml", "xml"),
    ("video/matroska", "mkv"),
    ("video/mp4", "mp4,m4v"),
    ("video/ogg", "ogv,ogm"),
    ("text/csv", "csv"),
    // kSecondaryMappings.
    ("image/x-icon", "ico"),
    ("application/epub+zip", "epub"),
    ("application/font-woff", "woff"),
    ("application/gzip", "gz,tgz"),
    ("application/javascript", "js"),
    ("application/json", "json"),
    ("application/msword", "doc,dot"),
    ("application/octet-stream", "bin,exe,com"),
    ("application/pdf", "pdf"),
    ("application/pkcs7-mime", "p7m,p7c,p7z"),
    ("application/pkcs7-signature", "p7s"),
    ("application/postscript", "ps,eps,ai"),
    ("application/rdf+xml", "rdf"),
    ("application/rss+xml", "rss"),
    ("application/rtf", "rtf"),
    ("application/vnd.android.package-archive", "apk"),
    ("application/vnd.mozilla.xul+xml", "xul"),
    ("application/vnd.ms-excel", "xls"),
    ("application/vnd.ms-powerpoint", "ppt"),
    (
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "pptx",
    ),
    (
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xlsx",
    ),
    (
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "docx",
    ),
    ("application/x-gzip", "gz,tgz"),
    ("application/x-mpegurl", "m3u8"),
    ("application/x-shockwave-flash", "swf,swl"),
    ("application/x-tar", "tar"),
    ("application/x-x509-ca-cert", "cer,crt"),
    ("application/zip", "zip"),
    ("audio/webm", "weba"),
    ("image/bmp", "bmp"),
    ("image/jpeg", "jfif,pjpeg,pjp"),
    ("image/tiff", "tiff,tif"),
    ("image/vnd.microsoft.icon", "ico"),
    ("image/x-png", "png"),
    ("image/x-xbitmap", "xbm"),
    ("message/rfc822", "eml"),
    ("text/calendar", "ics"),
    ("text/html", "ehtml"),
    ("text/markdown", "md"),
    ("text/plain", "txt,text"),
    ("text/vtt", "vtt"),
    ("text/x-sh", "sh"),
    ("text/xml", "xsl,xbl,xslt"),
    ("video/mpeg", "mpeg,mpg,mpe"),
];

/// A picked file's type as Chromium names it by its name's last extension
/// (after the last `.`), from `CHROMIUM_TYPES`; none (the browser's
/// empty `type`) for a name without one or an extension neither list has.
pub fn type_of_name(name: &str) -> Option<&'static str> {
    let extension = name.rsplit_once('.')?.1;
    CHROMIUM_TYPES.iter().find_map(|(mime, extensions)| {
        extensions
            .split(',')
            .any(|e| e.eq_ignore_ascii_case(extension))
            .then_some(*mime)
    })
}
// end of par-chat's block

/// The pictures the page redraws (`readUpload`'s list).
fn redrawn(content_type: &str) -> bool {
    matches!(
        content_type,
        "image/jpeg" | "image/png" | "image/gif" | "image/webp"
    )
}

/// A file read for sending (`readUpload`): an empty one or one over 100 MB
/// is refused with the page's text; a JPEG, PNG, GIF or WebP is redrawn
/// ([`paint_upload`]) and sent as JPEG under its own name; any other is sent
/// as it is, typed `application/octet-stream` when its type is not known.
/// `size` stays the file's own size.
pub fn read_upload(
    name: &str,
    content_type: Option<&str>,
    bytes: Vec<u8>,
) -> Result<OutgoingFile, String> {
    let size = bytes.len() as u64;
    if size == 0 {
        return Err(format!("\"{name}\" is empty"));
    }
    if size > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "\"{name}\" exceeds {} MB",
            megabytes(MAX_ATTACHMENT_BYTES)
        ));
    }
    let content_type = content_type.unwrap_or("application/octet-stream");
    if !redrawn(content_type) {
        return Ok(OutgoingFile {
            name: name.to_string(),
            content_type: content_type.to_string(),
            size,
            data: Arc::from(bytes),
        });
    }
    let picture = decode_upload(&bytes)
        // par-chat: the page's `createImageBitmap` rejection (Chromium's
        // `ImageBitmapFactories`); the decoder's own reason goes nowhere.
        .map_err(|_| UNDECODABLE.to_string())?;
    let painted = paint_upload(&picture, MAX_IMAGE_EDGE);
    let jpeg =
        encode_jpeg(&painted).map_err(|e| format!("\"{name}\" could not be made a JPEG: {e}"))?;
    Ok(OutgoingFile {
        name: name.to_string(),
        content_type: "image/jpeg".to_string(),
        size,
        data: Arc::from(jpeg),
    })
}

/// The redrawn picture as the page's canvas encodes it
/// (`toDataURL('image/jpeg', 0.88)`, Chromium's libjpeg-turbo): quality 88,
/// chroma subsampled 4:2:0 by averaging each 2 by 2 block, as libjpeg's
/// `h2v2_downsample` does; the standard Huffman tables, as the canvas
/// writes them (optimised tables made files 6 to 59 % smaller than the
/// browser's, further from it).
pub fn encode_jpeg(picture: &image::RgbImage) -> Result<Vec<u8>, String> {
    let (width, height) = picture.dimensions();
    let narrow = |side: u32| {
        u16::try_from(side).map_err(|_| format!("a side of {side} pixels is too long for JPEG"))
    };
    let mut jpeg = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut jpeg, JPEG_QUALITY);
    encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
    encoder.set_chroma_subsampling_method(jpeg_encoder::ChromaSubsamplingMethod::Average);
    encoder
        .encode(
            picture.as_raw(),
            narrow(width)?,
            narrow(height)?,
            jpeg_encoder::ColorType::Rgb,
        )
        .map_err(|e| e.to_string())?;
    Ok(jpeg)
}

// par-chat (stage four's parity pass)
/// A picture that cannot be decoded, in the page's words: Chromium's
/// `createImageBitmap` rejection (`third_party/blink/renderer/modules/
/// canvas/imagebitmap/image_bitmap_factories.cc`), shown by `showError`.
pub const UNDECODABLE: &str = "The source image could not be decoded.";

// chat-fix (W5): a pasted picture as the browser gives it.
/// The name a browser gives a picture pasted from the clipboard.
pub const PASTED_NAME: &str = "image.png";

/// A picture pasted from the clipboard as the page receives it: the
/// browser hands a pasted bitmap over as a PNG file named `image.png`
/// (`clipboardData.items`, `getAsFile()`), whatever the platform put on
/// the clipboard (a DIB on Windows, TIFF or PNG on macOS, PNG on Linux).
/// So any picture format is decoded and written as PNG under that name;
/// [`read_upload`] then redraws it as for any picked PNG (the model gets
/// `image.png` typed `image/jpeg`). A PNG is kept as it is.
pub fn pasted_picture(mime_type: &str, bytes: Vec<u8>) -> Result<(String, Vec<u8>), String> {
    if mime_type == "image/png" {
        return Ok((PASTED_NAME.to_string(), bytes));
    }
    let picture = decode_upload(&bytes)
        // par-chat: the page reads a pasted picture as a picked one.
        .map_err(|_| UNDECODABLE.to_string())?;
    let mut png = Vec::new();
    picture
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("The pasted picture could not be made a PNG: {e}"))?;
    Ok((PASTED_NAME.to_string(), png))
}

/// A file at `path` read for sending ([`read_upload`]), typed by its name.
pub fn read_path(path: &Path) -> Result<OutgoingFile, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let bytes = std::fs::read(path).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
    read_upload(&name, type_of_name(&name), bytes)
}

/// A picture's pixels as the page's `createImageBitmap(file)` gives them:
/// its first frame (an animated GIF's first), turned as its EXIF
/// orientation says (the bitmap's default `imageOrientation: "from-image"`).
/// Embedded colour profiles are not applied (the browser converts them to
/// sRGB; inventory README).
pub fn decode_upload(bytes: &[u8]) -> image::ImageResult<image::RgbaImage> {
    use image::ImageDecoder as _;
    let mut decoder = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut picture = image::DynamicImage::from_decoder(decoder)?;
    picture.apply_orientation(orientation);
    Ok(picture.to_rgba8())
}

/// An attached picture as the model gets it (`paintUpload`): a canvas of
/// `round(width × scale)` by `round(height × scale)`, `scale` being
/// `min(1, max_edge / longest edge)`, filled white, the picture drawn over
/// it (`drawImage`, source-over). A picture within the limit is drawn at its
/// own size, pixel for pixel; a larger one is resized in premultiplied alpha
/// with `image`'s triangle (linear) filter, whose support widens with the
/// reduction: of the filters measured against Chromium's canvas it came
/// closest (inventory README). Transparency arrives white, not black (JPEG
/// has no alpha).
pub fn paint_upload(picture: &image::RgbaImage, max_edge: u32) -> image::RgbImage {
    let (width, height) = picture.dimensions();
    let scale = (f64::from(max_edge) / f64::from(width.max(height))).min(1.);
    let w = ((f64::from(width) * scale).round() as u32).max(1);
    let h = ((f64::from(height) * scale).round() as u32).max(1);
    let premultiplied = image::Rgba32FImage::from_fn(width, height, |x, y| {
        // `f32` pixels are in [0, 1] (the resize clamps to it).
        let [r, g, b, a] = picture.get_pixel(x, y).0.map(|c| f32::from(c) / 255.);
        image::Rgba([r * a, g * a, b * a, a])
    });
    let drawn = if (w, h) == (width, height) {
        premultiplied
    } else {
        image::imageops::resize(&premultiplied, w, h, image::imageops::FilterType::Triangle)
    };
    image::RgbImage::from_fn(w, h, |x, y| {
        let [r, g, b, a] = drawn.get_pixel(x, y).0;
        // Source-over onto white.
        let over = |c: f32| ((c + 1. - a) * 255.).round().clamp(0., 255.) as u8;
        image::Rgb([over(r), over(g), over(b)])
    })
}

#[cfg(test)]
mod tests {
    //! `test/upload-image.test.js`, and the limits' texts.
    use super::*;

    #[test]
    fn a_pasted_bitmap_arrives_as_the_page_gets_it_image_png_then_redrawn() {
        // Windows puts a DIB on the clipboard (GPUI: image/bmp): the page's
        // browser gives `image.png`; the model gets it redrawn as JPEG under
        // that name.
        let picture = image::RgbImage::from_pixel(20, 10, image::Rgb([10, 200, 30]));
        let mut bmp = Vec::new();
        picture
            .write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp)
            .expect("a BMP");
        let (name, png) = pasted_picture("image/bmp", bmp).expect("read");
        assert_eq!(name, "image.png");
        assert_eq!(&png[1..4], b"PNG");
        let sent = read_upload(&name, Some("image/png"), png).expect("sent");
        assert_eq!(sent.name, "image.png");
        assert_eq!(sent.content_type, "image/jpeg");
        // A PNG is kept as it is; a picture that cannot be read says so.
        let (name, kept) = pasted_picture("image/png", vec![1, 2, 3]).expect("kept");
        assert_eq!((name.as_str(), kept), ("image.png", vec![1, 2, 3]));
        assert_eq!(
            pasted_picture("image/bmp", vec![0; 8]),
            Err(UNDECODABLE.to_string())
        );
        // par-chat: a picked picture that cannot be decoded, in the page's words.
        assert_eq!(
            read_upload("a.png", type_of_name("a.png"), vec![1, 2, 3]).map(|f| f.name),
            Err(UNDECODABLE.to_string())
        );
    }

    #[test]
    fn a_redrawn_picture_is_a_baseline_jpeg_with_chroma_at_4_2_0() {
        // The frame header (SOF0): three components, luma sampled 2 by 2,
        // each chroma 1 by 1: 4:2:0, as the page's canvas writes.
        let picture = image::RgbImage::from_fn(33, 17, |x, y| {
            image::Rgb([(x * 7) as u8, (y * 13) as u8, ((x ^ y) * 5) as u8])
        });
        let jpeg = encode_jpeg(&picture).expect("a JPEG");
        let at = jpeg
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("a baseline frame header");
        let sof = &jpeg[at + 2..];
        assert_eq!(u16::from_be_bytes([sof[3], sof[4]]), 17, "height");
        assert_eq!(u16::from_be_bytes([sof[5], sof[6]]), 33, "width");
        assert_eq!(sof[7], 3, "three components");
        let factors: Vec<u8> = (0..3).map(|c| sof[8 + c * 3 + 1]).collect();
        assert_eq!(factors, [0x22, 0x11, 0x11]);
        let decoded = image::load_from_memory(&jpeg)
            .expect("it decodes")
            .to_rgb8();
        assert_eq!(decoded.dimensions(), (33, 17));
    }

    #[test]
    fn an_upload_is_painted_on_white_before_the_image_so_transparency_is_not_black() {
        let sprite = image::RgbaImage::from_pixel(64, 32, image::Rgba([0, 0, 0, 0]));
        let painted = paint_upload(&sprite, MAX_IMAGE_EDGE);
        assert_eq!(painted.dimensions(), (64, 32));
        assert!(painted.pixels().all(|p| p.0 == [255, 255, 255]));
        // Opaque pixels keep their colour.
        let red = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 10, 10, 255]));
        assert_eq!(
            paint_upload(&red, MAX_IMAGE_EDGE).get_pixel(0, 0).0,
            [200, 10, 10]
        );
    }

    #[test]
    fn an_upload_longer_than_the_edge_limit_scales_down_whole_and_the_white_fills_all_of_it() {
        let logo = image::RgbaImage::from_pixel(4000, 1000, image::Rgba([0, 0, 0, 0]));
        let painted = paint_upload(&logo, MAX_IMAGE_EDGE);
        assert_eq!(painted.dimensions(), (1568, 392));
        assert!(painted.pixels().all(|p| p.0 == [255, 255, 255]));
    }

    #[test]
    fn a_picture_is_sent_as_jpeg_under_its_own_name_and_other_files_as_they_are() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(3000, 10, image::Rgba([0, 0, 0, 0]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("a PNG");
        let size = png.len() as u64;
        let file = read_upload("a.png", type_of_name("a.png"), png).expect("read");
        assert_eq!(
            (file.name.as_str(), file.content_type.as_str(), file.size),
            ("a.png", "image/jpeg", size)
        );
        let decoded = image::load_from_memory(&file.data).expect("a JPEG");
        assert_eq!((decoded.width(), decoded.height()), (1568, 5));
        let text =
            read_upload("notes.bin", type_of_name("notes.bin"), b"abc".to_vec()).expect("read");
        assert_eq!(text.content_type, "application/octet-stream");
        assert_eq!(&*text.data, b"abc");
        assert_eq!(
            read_upload("empty.txt", Some("text/plain"), Vec::new()),
            Err("\"empty.txt\" is empty".to_string())
        );
    }

    /// A JPEG of `width` by `height` whose EXIF says orientation 6 (turn
    /// 90° clockwise to show), its APP1 segment written by hand.
    fn jpeg_turned(width: u32, height: u32) -> Vec<u8> {
        let mut plain = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut plain, 90)
            .encode_image(&image::RgbImage::from_pixel(
                width,
                height,
                image::Rgb([200, 30, 30]),
            ))
            .expect("a JPEG");
        // TIFF, little-endian: one IFD entry, Orientation (0x0112), SHORT, 1, 6.
        let tiff: Vec<u8> = [
            &b"II*\0"[..],
            &8u32.to_le_bytes(),
            &1u16.to_le_bytes(),
            &0x0112u16.to_le_bytes(),
            &3u16.to_le_bytes(),
            &1u32.to_le_bytes(),
            &6u16.to_le_bytes(),
            &[0, 0],
            &0u32.to_le_bytes(),
        ]
        .concat();
        let body = [&b"Exif\0\0"[..], &tiff].concat();
        let length = (body.len() + 2) as u16;
        [
            &plain[..2],
            &[0xFF, 0xE1],
            &length.to_be_bytes(),
            &body,
            &plain[2..],
        ]
        .concat()
    }

    fn png(picture: &image::RgbaImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        picture
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("a PNG");
        bytes
    }

    fn sent(name: &str, bytes: Vec<u8>) -> image::RgbImage {
        let file = read_upload(name, type_of_name(name), bytes).expect("read");
        assert_eq!(file.content_type, "image/jpeg");
        image::load_from_memory(&file.data)
            .expect("the output decodes")
            .to_rgb8()
    }

    fn near(a: [u8; 3], b: [u8; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 6)
    }

    #[test]
    fn authored_pictures_arrive_as_the_page_sends_them() {
        // A large photo-like gradient: scaled whole to 1568 on its long edge.
        let photo = image::RgbaImage::from_fn(4032, 3024, |x, y| {
            image::Rgba([(x * 255 / 4031) as u8, (y * 255 / 3023) as u8, 128, 255])
        });
        let out = sent("photo.png", png(&photo));
        assert_eq!(out.dimensions(), (1568, 1176));
        assert!(
            near(out.get_pixel(784, 588).0, [127, 127, 128]),
            "{:?}",
            out.get_pixel(784, 588)
        );
        // A transparent PNG: transparent is white, half red over white is pink.
        let clear = image::RgbaImage::from_fn(300, 200, |x, _| {
            if x < 150 {
                image::Rgba([0, 0, 0, 0])
            } else {
                image::Rgba([255, 0, 0, 128])
            }
        });
        let out = sent("clear.png", png(&clear));
        assert_eq!(out.dimensions(), (300, 200));
        assert!(
            near(out.get_pixel(40, 100).0, [255, 255, 255]),
            "{:?}",
            out.get_pixel(40, 100)
        );
        assert!(
            near(out.get_pixel(260, 100).0, [255, 127, 127]),
            "{:?}",
            out.get_pixel(260, 100)
        );
        // A small picture keeps its size.
        let small = image::RgbaImage::from_pixel(64, 48, image::Rgba([10, 120, 200, 255]));
        let out = sent("small.png", png(&small));
        assert_eq!(out.dimensions(), (64, 48));
        assert!(near(out.get_pixel(32, 24).0, [10, 120, 200]));
        // A JPEG turned by its EXIF orientation arrives upright.
        let out = sent("turned.jpg", jpeg_turned(40, 20));
        assert_eq!(out.dimensions(), (20, 40));
        assert!(near(out.get_pixel(10, 20).0, [200, 30, 30]));
    }

    #[test]
    fn the_limits_count_every_file_of_the_draft_read_or_not() {
        let file = |size| ("f".to_string(), size);
        assert_eq!(
            check_attachment_limits(&vec![file(1); 7], &[file(1), file(1)]),
            Err("A message supports at most 8 attachments".to_string())
        );
        assert_eq!(
            check_attachment_limits(&vec![file(1); 7], &[file(1)]),
            Ok(())
        );
        assert_eq!(
            check_attachment_limits(
                &[file(MAX_ATTACHMENT_BYTES)],
                &[file(MAX_ATTACHMENT_BYTES), file(1)]
            ),
            Err("Attachments exceed 200 MB total".to_string())
        );
        assert_eq!(
            read_upload("big.bin", None, vec![0; MAX_ATTACHMENT_BYTES as usize + 1]),
            Err("\"big.bin\" exceeds 100 MB".to_string())
        );
    }

    // par-chat: the type table, case for case against Chromium's lists.
    #[test]
    fn a_picked_file_is_typed_as_chromium_types_it_by_its_last_extension() {
        for (name, mime) in [
            ("a.js", Some("text/javascript")),
            ("a.mjs", Some("text/javascript")),
            ("a.xml", Some("text/xml")),
            ("a.mp4", Some("video/mp4")),
            ("a.webm", Some("video/webm")),
            ("a.mp3", Some("audio/mpeg")),
            ("a.PNG", Some("image/png")),
            ("a.apng", Some("image/apng")),
            ("a.jpe", Some("image/jpeg")),
            ("a.jfif", Some("image/jpeg")),
            ("a.pjp", Some("image/jpeg")),
            ("a.ico", Some("image/x-icon")),
            ("a.gz", Some("application/gzip")),
            ("a.exe", Some("application/octet-stream")),
            ("a.md", Some("text/markdown")),
            ("a.text", Some("text/plain")),
            ("a.svgz", Some("image/svg+xml")),
            ("archive.tar.gz", Some("application/gzip")),
            ("a.py", None),
            ("a.yaml", None),
            ("a.", None),
            ("Makefile", None),
        ] {
            assert_eq!(type_of_name(name), mime, "{name}");
        }
    }

    #[test]
    fn a_jfif_picture_is_redrawn_as_the_page_redraws_a_jpeg() {
        let jpeg = encode_jpeg(&image::RgbImage::from_pixel(4, 4, image::Rgb([10, 20, 30])))
            .expect("jpeg");
        let file = read_upload("a.jfif", type_of_name("a.jfif"), jpeg).expect("read");
        assert_eq!(file.content_type, "image/jpeg");
        assert_eq!(file.name, "a.jfif");
    }
}
