//! The shared viewer and the page of one shared build (`server/share-viewer.js`):
//! `viewer/<viewerId>/` of a person's site ([`write_viewer`]) and each
//! share's `b/<shareId>/index.html` ([`share_page_html`]), and BlueMap's web
//! app the viewer is made of, read from BlueMap's pinned CLI jar without
//! Java ([`bluemap_web_app`]).
//!
//! A viewer folder holds BlueMap's web app (`index.html`, `assets/` without
//! the source map, `lang/`), the map (its settings, textures and the empty
//! live marker files BlueMap polls), the pack and the mesher gzipped
//! (GitHub Pages compresses no binary file, and the worker reads `.gz` as
//! it streams), the app's own mesher modules and `web/share/bridge.js`.
//! Each file's source in the native app:
//!
//! | In the viewer folder | Source |
//! |---|---|
//! | `index.html`, `assets/`, `lang/` | the web app folder [`bluemap_web_app`] unpacked from BlueMap's jar (fetched at the first publish and kept in the data folder) |
//! | `maps/live/live/markers.json`, `players.json` | `{}`, as the bake writes them |
//! | `maps/live/textures.json` | setup's `textures/<id>/textures.json` |
//! | `mcbench-pack.bin.gz` | `minecraft/<id>/mcbench-pack.bin`, gzipped |
//! | `settings.json`, `maps/live/settings.json` | `minecraft/<id>/settings.json` and `map-settings.json`, rewritten for a share |
//! | `mesher.wasm.gz` | `renderer/mcbench-renderer.wasm`, embedded at build time, gzipped |
//! | `mesher.mjs`, `mesher-worker.mjs`, `bluemap-tiles.mjs`, `bluemap-view.mjs`, `share/bridge.js` | `web/`, embedded at build time |
//!
//! Only the owner's code (the WebAssembly and the five modules) is embedded
//! in the program; nothing of BlueMap's or Mojang's is.
//!
//! The gzip members are flate2's at level 9, whose bytes are not zlib's, so
//! the viewer id of the same content differs from today's Node server's: a
//! site then holds one more viewer folder, and earlier shares keep theirs.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use mcbench_setup::jar;
use mcbench_setup::pins::{self, Download};
use serde_json::Value;
use sha2::Digest;
use tokio_util::sync::CancellationToken;

/// The app's modules the share bridge draws with, and the bridge, by their
/// path in the viewer folder, embedded at build time from this crate's
/// `share-page/` folder: the owner's own code, copies of the localhost app's
/// `web/` files of the same names (a test holds them equal while `web/` is
/// in the tree; the checks' trees carry `native/` without `web/`).
pub const CODE: [(&str, &[u8]); 5] = [
    ("mesher.mjs", include_bytes!("../share-page/mesher.mjs")),
    (
        "mesher-worker.mjs",
        include_bytes!("../share-page/mesher-worker.mjs"),
    ),
    (
        "bluemap-tiles.mjs",
        include_bytes!("../share-page/bluemap-tiles.mjs"),
    ),
    (
        "bluemap-view.mjs",
        include_bytes!("../share-page/bluemap-view.mjs"),
    ),
    (
        "share/bridge.js",
        include_bytes!("../share-page/share/bridge.js"),
    ),
];

/// The mesher's WebAssembly (`renderer/mcbench-renderer.wasm`), embedded
/// at build time.
pub const MESHER: &[u8] = include_bytes!("../../../../renderer/mcbench-renderer.wasm");

/// What of BlueMap's web app the shared viewer uses, as it is: its page,
/// scripts, styles and languages.
const WEB_APP: [&str; 3] = ["index.html", "assets", "lang"];

/// The files a viewer folder is made of besides the embedded code.
#[derive(Clone, Debug)]
pub struct ViewerParts {
    /// BlueMap's web app, unpacked ([`bluemap_web_app`]).
    pub web_app: PathBuf,
    /// BlueMap's `textures.json` of the map.
    pub textures: PathBuf,
    /// The pack (`mcbench-pack.bin`).
    pub pack: PathBuf,
    /// BlueMap's web app settings (`settings.json`).
    pub settings: PathBuf,
    /// The map's settings (`maps/<map>/settings.json`).
    pub map_settings: PathBuf,
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn files_under(
    root: &Path,
    relative: &str,
    into: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    let at = root.join(relative);
    let meta = std::fs::metadata(&at).map_err(|e| format!("{}: {e}", at.display()))?;
    if meta.is_file() {
        // BlueMap's source map is for debugging BlueMap, 3.5 MB of it.
        if !relative.ends_with(".map") {
            into.insert(relative.to_string(), read(&at)?);
        }
        return Ok(());
    }
    let mut names = std::fs::read_dir(&at)
        .map_err(|e| format!("{}: {e}", at.display()))?
        .map(|entry| {
            entry
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .map_err(|e| format!("{}: {e}", at.display()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    for name in names {
        files_under(root, &format!("{relative}/{name}"), into)?;
    }
    Ok(())
}

/// `zlib.gzipSync(bytes, { level: 9 })`'s format: one gzip member at the
/// best compression.
pub fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(bytes).expect("writing to memory");
    encoder.finish().expect("writing to memory")
}

fn parse_object(bytes: &[u8], path: &Path) -> Result<serde_json::Map<String, Value>, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8", path.display()))?;
    match mcbench_json::parse(text).map_err(|e| format!("{}: {e}", path.display()))? {
        Value::Object(object) => Ok(object),
        _ => Err(format!("{} is not a JSON object", path.display())),
    }
}

/// The viewer folder's files, as published path to bytes (`viewerFiles`).
pub fn viewer_files(parts: &ViewerParts) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    for name in WEB_APP {
        files_under(&parts.web_app, name, &mut files)?;
    }
    // BlueMap loads the share bridge, and keeps no camera or settings in
    // cookies: a share opens as it was shared. It loads no tile before the
    // bridge runs (view distances 0): the site has no tile files, and a static
    // host answers each with a 404 the console shows. The bridge then loads
    // hires over the whole build (bluemap-view.mjs) and lowres never.
    let mut settings = parse_object(&read(&parts.settings)?, &parts.settings)?;
    let map = settings
        .get("maps")
        .and_then(|maps| maps.get(0))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{} names no map", parts.settings.display()))?
        .to_string();
    settings.insert("useCookies".into(), Value::Bool(false));
    settings.insert("hiresSliderDefault".into(), 0.into());
    settings.insert("lowresSliderMin".into(), 0.into());
    settings.insert("lowresSliderDefault".into(), 0.into());
    settings.insert(
        "scripts".into(),
        Value::Array(vec!["share/bridge.js".into()]),
    );
    settings.insert("styles".into(), Value::Array(Vec::new()));
    files.insert(
        "settings.json".into(),
        mcbench_json::stringify(&Value::Object(settings)).into_bytes(),
    );
    // Below the horizon BlueMap draws the map's void colour, black: in a share
    // it is the sky's colour, so the build stands in sky at every camera angle.
    let mut map_settings = parse_object(&read(&parts.map_settings)?, &parts.map_settings)?;
    let sky = map_settings
        .get("skyColor")
        .cloned()
        .ok_or_else(|| format!("{} has no skyColor", parts.map_settings.display()))?;
    map_settings.insert("voidColor".into(), sky);
    files.insert(
        format!("maps/{map}/settings.json"),
        mcbench_json::stringify(&Value::Object(map_settings)).into_bytes(),
    );
    files.insert(format!("maps/{map}/textures.json"), read(&parts.textures)?);
    // The live marker files BlueMap polls, empty as the bake writes them.
    files.insert(format!("maps/{map}/live/markers.json"), b"{}".to_vec());
    files.insert(format!("maps/{map}/live/players.json"), b"{}".to_vec());
    files.insert("mcbench-pack.bin.gz".into(), gzip(&read(&parts.pack)?));
    files.insert("mesher.wasm.gz".into(), gzip(MESHER));
    for (name, bytes) in CODE {
        files.insert(name.into(), bytes.to_vec());
    }
    Ok(files)
}

/// The id of a viewer folder's files: a hash of every path and byte in it,
/// in path order (`writeViewer`).
pub fn viewer_id(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut hash = sha2::Sha256::new();
    for (name, bytes) in files {
        hash.update(format!("{name}\n{}\n", jar::hex(&sha2::Sha256::digest(bytes))).as_bytes());
    }
    jar::hex(&hash.finalize())[..16].to_string()
}

/// Writes `site/viewer/<viewerId>/` unless it is there, and returns its id
/// (`writeViewer`): a changed web app, map or app code is a new folder, and
/// the pages of earlier shares keep the viewer they were made with. The
/// folder appears whole (a staging folder, renamed).
pub fn write_viewer(site: &Path, parts: &ViewerParts) -> Result<String, String> {
    let files = viewer_files(parts)?;
    let id = viewer_id(&files);
    let folder = site.join("viewer").join(&id);
    if folder.exists() {
        return Ok(id);
    }
    let staging = PathBuf::from(format!(
        "{}.{}.partial",
        folder.display(),
        std::process::id()
    ));
    remove(&staging)?;
    for (name, bytes) in &files {
        let path = staging.join(name);
        let parent = path.parent().expect("a file in the staging folder");
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    std::fs::rename(&staging, &folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    Ok(id)
}

fn remove(path: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}: {e}", path.display()))
        }
        _ => Ok(()),
    }
}

/// `escapeHtml`: `&`, `<`, `>`, `"` and `'` as entities.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// The page of a share, `b/<shareId>/index.html` (`sharePageHtml`), byte
/// for byte. Its tags are what X's crawler (which runs no JavaScript) and
/// other link previews read: X shows `url` itself, this page, in a 480x480
/// player frame. The page is the viewer filling the window, under a cover
/// until the first build is on screen (the share bridge tells it), with a
/// line along the bottom while the rest is drawn, or saying why it could not
/// be shown. `labels` name the share's builds in order; with more than one,
/// a strip of them along the top switches the viewer between them (the left
/// and right arrow keys too). The labels are in the markup only, as escaped
/// pill text, which the script reads back. Bottom left, the camera's mode
/// (Fly: BlueMap's free flight; Orbit: its map controls) and a button back
/// to the opening view; a hint of the mode's controls on this device shows
/// when the build first appears and when the mode changes, and fades.
pub fn share_page_html(
    viewer_id: &str,
    share_id: &str,
    title: &str,
    description: &str,
    url: &str,
    image: &str,
    labels: &[String],
) -> String {
    let t = escape_html(title);
    let d = escape_html(description);
    let u = escape_html(url);
    let i = escape_html(image);
    let v = escape_html(viewer_id);
    let s = escape_html(share_id);
    let strip = if labels.len() > 1 {
        let pills: String = labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                format!(
                    r#"<button type="button" class="pill" data-index="{index}" aria-pressed="{}">{}</button>"#,
                    index == 0,
                    escape_html(label)
                )
            })
            .collect();
        format!(
            "<nav id=\"builds\" aria-label=\"Builds\"><div><button type=\"button\" id=\"prev\" aria-label=\"Previous build\">&lsaquo;</button><div id=\"pills\">{pills}</div><button type=\"button\" id=\"next\" aria-label=\"Next build\">&rsaquo;</button></div></nav>\n"
        )
    } else {
        String::new()
    };
    let mut html = String::with_capacity(16 * 1024);
    html.push_str(r###"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no">
<title>"###);
    html.push_str(&t);
    html.push_str(
        r###"</title>
<meta name="description" content=""###,
    );
    html.push_str(&d);
    html.push_str(
        r###"">
<meta name="twitter:card" content="player">
<meta name="twitter:title" content=""###,
    );
    html.push_str(&t);
    html.push_str(
        r###"">
<meta name="twitter:description" content=""###,
    );
    html.push_str(&d);
    html.push_str(
        r###"">
<meta name="twitter:image" content=""###,
    );
    html.push_str(&i);
    html.push_str(
        r###"">
<meta name="twitter:player" content=""###,
    );
    html.push_str(&u);
    html.push_str(
        r###"">
<meta name="twitter:player:width" content="480">
<meta name="twitter:player:height" content="480">
<meta property="og:title" content=""###,
    );
    html.push_str(&t);
    html.push_str(
        r###"">
<meta property="og:description" content=""###,
    );
    html.push_str(&d);
    html.push_str(
        r###"">
<meta property="og:image" content=""###,
    );
    html.push_str(&i);
    html.push_str(
        r###"">
<meta property="og:url" content=""###,
    );
    html.push_str(&u);
    html.push_str(r###"">
<meta property="og:type" content="website">
<style>
html,body{margin:0;height:100%;overflow:hidden;background:#7dabff;font:14px/1.4 system-ui,-apple-system,"Segoe UI",sans-serif}
#viewer{position:fixed;inset:0;width:100%;height:100%;border:0;display:block}
#cover{position:fixed;inset:0;display:flex;align-items:center;justify-content:center;padding:16px;box-sizing:border-box;text-align:center;background:#16181d;color:#e8e6e1;transition:opacity .3s}
#cover.shown{opacity:0;pointer-events:none}
#cover.failed{color:#ffb4a8}
#full{position:fixed;right:8px;bottom:8px;padding:4px 9px;border-radius:6px;background:rgba(12,12,12,.7);color:#f2efe8;font-size:12px;text-decoration:none}
#full:hover{background:rgba(12,12,12,.9)}
#progress{position:fixed;left:0;right:0;bottom:0;height:3px;background:rgba(12,12,12,.25);pointer-events:none}
#progress>div{height:100%;width:0;background:#f2efe8;transition:width .2s}
#progress>span{position:absolute;left:136px;bottom:8px;padding:2px 7px;border-radius:5px;background:rgba(12,12,12,.55);color:#f2efe8;font-size:11px}
#builds{position:fixed;top:8px;left:8px;right:8px;display:flex;justify-content:center;pointer-events:none}
#builds>div{display:flex;align-items:center;gap:2px;max-width:100%;padding:3px;box-sizing:border-box;border-radius:8px;background:rgba(12,12,12,.7);pointer-events:auto}
#pills{position:relative;display:flex;gap:3px;overflow-x:auto;scrollbar-width:none;scroll-behavior:smooth;min-width:0}
#pills::-webkit-scrollbar{display:none}
#builds button{flex:none;border:0;margin:0;font:inherit;font-size:12px;line-height:18px;color:#f2efe8;background:transparent;cursor:pointer;border-radius:5px}
#builds button:focus-visible{outline:2px solid #f2efe8;outline-offset:-2px}
#builds .pill{padding:3px 10px;white-space:nowrap;background:rgba(255,255,255,.08)}
#builds .pill:hover{background:rgba(255,255,255,.18)}
#builds .pill[aria-pressed="true"]{background:#f2efe8;color:#16181d;font-weight:600}
#prev,#next{width:26px;padding:3px 0;font-size:17px!important}
#view{position:fixed;left:8px;bottom:8px;display:flex;gap:2px;padding:2px;border-radius:6px;background:rgba(12,12,12,.7)}
#view button{display:flex;align-items:center;border:0;margin:0;padding:2px 8px;border-radius:4px;font:inherit;font-size:12px;line-height:17px;color:#f2efe8;background:transparent;cursor:pointer}
#view button:hover{background:rgba(255,255,255,.14)}
#view button[aria-pressed="true"]{background:#f2efe8;color:#16181d;font-weight:600}
#view button:focus-visible{outline:2px solid #f2efe8;outline-offset:-2px}
#view svg{width:15px;height:15px;fill:none;stroke:currentColor;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}
#reset{padding:2px 6px!important}
#hint{position:fixed;left:50%;bottom:44px;transform:translateX(-50%);width:max-content;max-width:calc(100% - 32px);box-sizing:border-box;padding:5px 10px;border-radius:6px;background:rgba(12,12,12,.7);color:#f2efe8;font-size:12px;text-align:center;pointer-events:none;opacity:0;transition:opacity .5s}
#hint.on{opacity:1}
#prev:disabled,#next:disabled{opacity:.35;cursor:default}
</style>
</head>
<body>
<iframe id="viewer" src="../../viewer/"###);
    html.push_str(&v);
    html.push_str(r###"/?build=../../b/"###);
    html.push_str(&s);
    html.push_str(r###"/build.json" title=""###);
    html.push_str(&t);
    html.push_str(r###"" allow="fullscreen"></iframe>
<div id="cover" role="status">Loading the build…</div>
<div id="progress" role="progressbar" hidden><div></div><span></span></div>
<a id="full" href="./" target="_blank" rel="noopener">Open full screen</a>
<div id="view"><button type="button" data-mode="fly" aria-pressed="false" title="Fly: move freely through the build">Fly</button><button type="button" data-mode="orbit" aria-pressed="false" title="Orbit: turn around the build">Orbit</button><button type="button" id="reset" title="Reset view" aria-label="Reset view"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 12a9 9 0 1 0 3-6.7"/><path d="M3 4v5h5"/></svg></button></div>
<div id="hint" role="note"></div>
"###);
    html.push_str(&strip);
    html.push_str(r###"<script>
(function () {
  var frame = document.getElementById('viewer');
  var cover = document.getElementById('cover');
  var progress = document.getElementById('progress');
  // The strip's pills, one per build (none for a share of one build).
  var pills = Array.prototype.slice.call(document.querySelectorAll('#builds .pill'));
  var prev = document.getElementById('prev');
  var next = document.getElementById('next');
  // The build asked for: what the bridge tells of any other is ignored.
  var wanted = 0;
  // Whether a build has been on screen: the cover is for the first load.
  var seen = false;
  // Opened on its own, the page is already the full screen.
  if (window.top === window) document.getElementById('full').hidden = true;
  function failed(index, text) {
    cover.className = 'failed';
    cover.textContent = (pills.length ? pills[index].textContent : 'The build') + ' could not be shown: ' + text;
    progress.hidden = true;
  }
  function line(drawn, total, text) {
    progress.hidden = false;
    progress.firstChild.style.width = (total ? 100 * drawn / total : 0) + '%';
    progress.lastChild.textContent = text;
  }
  // Highlights the pill of build index, scrolls it into the strip's view (the strip
  // alone: scrolling the page would scroll a post around X's frame) and
  // asks the bridge for that build; the bottom line says it is loading.
  function choose(index) {
    if (index === wanted) return;
    wanted = index;
    mark();
    if (!seen || cover.className === 'failed') {
      cover.className = '';
      cover.textContent = 'Loading ' + pills[index].textContent + '…';
    }
    line(0, 0, 'Loading ' + pills[index].textContent + '…');
    frame.contentWindow.postMessage({ type: 'mcbench-share:show', index: index }, window.location.origin);
  }
  function mark() {
    pills.forEach(function (pill, index) { pill.setAttribute('aria-pressed', String(index === wanted)); });
    prev.disabled = wanted === 0;
    next.disabled = wanted === pills.length - 1;
    var strip = pills[wanted].parentNode;
    var pill = pills[wanted];
    if (pill.offsetLeft < strip.scrollLeft) strip.scrollLeft = pill.offsetLeft;
    else if (pill.offsetLeft + pill.offsetWidth > strip.scrollLeft + strip.clientWidth) strip.scrollLeft = pill.offsetLeft + pill.offsetWidth - strip.clientWidth;
  }
  if (pills.length) {
    pills.forEach(function (pill, index) { pill.addEventListener('click', function () { choose(index); }); });
    prev.addEventListener('click', function () { if (wanted > 0) choose(wanted - 1); });
    next.addEventListener('click', function () { if (wanted < pills.length - 1) choose(wanted + 1); });
    // While the page has focus (the viewer takes the keys once clicked).
    document.addEventListener('keydown', function (event) {
      if (event.key === 'ArrowLeft' && wanted > 0) choose(wanted - 1);
      else if (event.key === 'ArrowRight' && wanted < pills.length - 1) choose(wanted + 1);
      else return;
      event.preventDefault();
    });
    mark();
  }
  // The share bridge says when the build is on screen or why it is not. A
  // viewer that never says (its page or BlueMap did not load) is a failure too.
  // The camera's mode, as the bridge last told it, and the hint of its
  // controls on this device (a finger only: the bridge opens in orbit, as
  // free flight moves by keys alone). Keys reach the viewer once it is
  // clicked: the page itself takes the arrows for the strip.
  var touch = window.matchMedia('(pointer: coarse) and (hover: none)').matches;
  var HINTS = touch
    ? { fly: 'Drag to look around · Orbit to move', orbit: 'Drag to move · Two fingers to turn · Pinch to zoom' }
    : { fly: 'Drag to look · Click, then W A S D to fly · Space / Shift up, down · Scroll for speed', orbit: 'Drag to move · Right-drag to turn · Scroll to zoom' };
  var mode = null;
  var hint = document.getElementById('hint');
  var hintTimer = null;
  function showHint() {
    hint.textContent = HINTS[mode];
    hint.className = 'on';
    clearTimeout(hintTimer);
    hintTimer = setTimeout(hideHint, 6000);
  }
  function hideHint() {
    hint.className = '';
  }
  // The first input, in the viewer (the bridge tells) or on this page,
  // ends the hint.
  document.addEventListener('pointerdown', hideHint);
  document.addEventListener('keydown', hideHint);
  var modes = Array.prototype.slice.call(document.querySelectorAll('#view [data-mode]'));
  function markMode(next) {
    var changed = mode !== null && next !== mode;
    mode = next;
    modes.forEach(function (button) { button.setAttribute('aria-pressed', String(button.dataset.mode === mode)); });
    if (changed && seen) showHint();
  }
  modes.forEach(function (button) {
    button.addEventListener('click', function () {
      markMode(button.dataset.mode);
      frame.contentWindow.postMessage({ type: 'mcbench-share:set-mode', mode: button.dataset.mode }, window.location.origin);
    });
  });
  document.getElementById('reset').addEventListener('click', function () {
    frame.contentWindow.postMessage({ type: 'mcbench-share:reset' }, window.location.origin);
  });
  var silence = setTimeout(function () { failed(wanted, 'the viewer did not load.'); }, 60000);
  window.addEventListener('message', function (event) {
    if (event.source !== frame.contentWindow || event.origin !== window.location.origin) return;
    clearTimeout(silence);
    if (event.data.type === 'mcbench-share:mode') return markMode(event.data.mode);
    if (event.data.type === 'mcbench-share:input') return hideHint();
    if (event.data.index !== wanted) return;
    if (event.data.type === 'mcbench-share:shown') {
      if (!seen) showHint();
      seen = true;
      cover.className = 'shown';
    }
    if (event.data.type === 'mcbench-share:failed') failed(event.data.index, event.data.error);
    // A line along the bottom while tiles are still being drawn.
    if (event.data.type === 'mcbench-share:progress') {
      line(event.data.drawn, event.data.total, 'Drawing ' + event.data.drawn + ' of ' + event.data.total);
      progress.hidden = event.data.drawn === event.data.total;
    }
  });
})();
</script>
</body>
</html>
"###);
    html
}

/// The folder of BlueMap's web app in the data folder `data`.
pub fn web_app_folder(data: &Path) -> PathBuf {
    data.join("bluemap")
        .join(format!("webapp-{}", pins::BLUEMAP_VERSION))
}

/// The web app's entry in BlueMap's CLI jar.
const WEB_APP_ZIP: &str = "de/bluecolored/bluemap/webapp.zip";

/// BlueMap's web app, unpacked into the data folder `data` and kept: at the
/// first call BlueMap's CLI jar is downloaded from `jar` (the app passes
/// [`pins::bluemap`]; a test, a local server) into the data folder and kept
/// (its sha256 checked, a partial download never left under the jar's
/// name), and the entries of the zip inside it written to
/// [`web_app_folder`], which appears whole. A later call fetches nothing.
pub async fn bluemap_web_app(
    data: &Path,
    client: &reqwest::Client,
    jar: &Download,
    cancel: &CancellationToken,
) -> Result<PathBuf, String> {
    let folder = web_app_folder(data);
    if folder.is_dir() {
        return Ok(folder);
    }
    let lock = PathBuf::from(format!("{}.lock", folder.display()));
    let file = data
        .join("bluemap")
        .join(format!("bluemap-{}-cli.jar", pins::BLUEMAP_VERSION));
    let done = folder.clone();
    pins::with_lock(&lock, async move {
        if folder.is_dir() {
            return Ok(());
        }
        if !file.is_file() {
            let none = |_: mcbench_setup::Bytes| {};
            tokio::select! {
                result = pins::download(client, &jar.url, &file, &jar.sha256, &none) => result.map_err(|e| {
                    format!(
                        "BlueMap {} could not be downloaded from GitHub for the shared page ({e}). Try again.",
                        pins::BLUEMAP_VERSION
                    )
                })?,
                () = cancel.cancelled() => {
                    // The download's own temporary file (pins::download).
                    let partial = PathBuf::from(format!("{}.{}.download", file.display(), std::process::id()));
                    let _ = std::fs::remove_file(partial);
                    return Err("Cancelled by user".into());
                }
            }
        }
        let staging = PathBuf::from(format!("{}.{}.partial", folder.display(), std::process::id()));
        let (from, to) = (file.clone(), staging.clone());
        let unpacked = tokio::task::spawn_blocking(move || unpack_web_app(&from, &to))
            .await
            .map_err(|e| format!("unpacking BlueMap's web app: {e}"))?;
        if let Err(e) = unpacked {
            remove(&staging)?;
            return Err(e);
        }
        std::fs::rename(&staging, &folder).map_err(|e| format!("{}: {e}", folder.display()))
    })
    .await?;
    Ok(done)
}

/// An entry's bytes from its stored bytes, its size and CRC32 checked.
fn inflate(entry: &jar::Entry, stored: &[u8]) -> Result<Vec<u8>, String> {
    let bytes = match entry.method {
        0 => stored.to_vec(),
        8 => {
            let mut out = Vec::with_capacity(entry.size as usize);
            flate2::read::DeflateDecoder::new(stored)
                .read_to_end(&mut out)
                .map_err(|e| format!("{} does not inflate: {e}", entry.name))?;
            out
        }
        other => return Err(format!("{} has compression method {other}", entry.name)),
    };
    if bytes.len() as u64 != entry.size || crc32fast::hash(&bytes) != entry.crc32 {
        return Err(format!(
            "{} is damaged (its size or CRC32 differs)",
            entry.name
        ));
    }
    Ok(bytes)
}

/// Writes every file of the web app zip in the jar `file` into `to`.
fn unpack_web_app(file: &Path, to: &Path) -> Result<(), String> {
    remove(to)?;
    let bytes = read(file)?;
    let entries = jar::entries(&bytes)?;
    let zip = entries
        .iter()
        .find(|e| e.name == WEB_APP_ZIP)
        .ok_or_else(|| format!("{} holds no {WEB_APP_ZIP}", file.display()))?;
    let zip = inflate(zip, jar::stored(&bytes, zip))?;
    for entry in jar::entries(&zip)? {
        if entry.name.ends_with('/') {
            continue;
        }
        let path = to.join(&entry.name);
        let parent = path.parent().expect("an entry in the folder");
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        std::fs::write(&path, inflate(&entry, jar::stored(&zip, &entry))?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}
