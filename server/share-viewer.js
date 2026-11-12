'use strict';

// The shared viewer: a static copy of the app's 3D viewer that GitHub Pages
// serves with no server behind it, and the page of one shared build.
//
// A site holds
//   viewer/<viewerId>/   BlueMap's web app as the bake made it (server/bluemap.js),
//                        the map's settings and textures, the pack and the
//                        mesher (gzip: GitHub Pages compresses no binary file,
//                        and the worker reads .gz as it streams), the app's
//                        own mesher modules, copied, and web/share/bridge.js
//   b/<shareId>/         a share: index.html (sharePageHtml), preview.jpg,
//                        build.json (the title, and one build for a chat or
//                        one per model for a bench: its label, blueprint,
//                        render mask and bounds) and 0.bin.gz, 1.bin.gz, …
//                        (each build's gzipped blueprint), which the share
//                        bridge reads
// The site sits under /<repo>/, so every address in it is relative.

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const WEB = path.join(__dirname, '..', 'web');
// What the shared viewer uses of the bake's web folder, as it is: BlueMap's
// page, scripts, styles and languages, and the map (its settings, textures
// and the live marker files it polls, which stay empty). Its entity atlas
// and sql.php are not used.
const BAKED = ['index.html', 'assets', 'lang', 'maps'];
// The app's modules the share bridge draws with, and the bridge.
const CODE = ['mesher.mjs', 'mesher-worker.mjs', 'bluemap-tiles.mjs', 'bluemap-view.mjs', 'share/bridge.js'];

function filesUnder(root, relative, into) {
  const at = path.join(root, relative);
  if (fs.statSync(at).isFile()) {
    // BlueMap's source map is for debugging BlueMap, 3.5 MB of it.
    if (!relative.endsWith('.map')) into.set(relative.split(path.sep).join('/'), fs.readFileSync(at));
    return;
  }
  for (const name of fs.readdirSync(at).sort()) filesUnder(root, path.join(relative, name), into);
}

// The viewer folder's files, as published path -> bytes.
function viewerFiles(webRoot, mesher) {
  const files = new Map();
  for (const name of BAKED) filesUnder(webRoot, name, files);
  // BlueMap loads the share bridge, and keeps no camera or settings in
  // cookies: a share opens as it was shared. It loads no tile before the
  // bridge runs (view distances 0): the site has no tile files, and a static
  // host answers each with a 404 the console shows. The bridge then loads
  // hires over the whole build (bluemap-view.mjs) and lowres never.
  const settings = JSON.parse(fs.readFileSync(path.join(webRoot, 'settings.json'), 'utf8'));
  files.set('settings.json', Buffer.from(JSON.stringify({
    ...settings, useCookies: false, hiresSliderDefault: 0, lowresSliderMin: 0, lowresSliderDefault: 0, scripts: ['share/bridge.js'], styles: [],
  })));
  // Below the horizon BlueMap draws the map's void colour, black: in a share
  // it is the sky's colour, so the build stands in sky at every camera angle.
  const map = `maps/${settings.maps[0]}/settings.json`;
  const mapSettings = JSON.parse(files.get(map).toString('utf8'));
  files.set(map, Buffer.from(JSON.stringify({ ...mapSettings, voidColor: mapSettings.skyColor })));
  files.set('mcbench-pack.bin.gz', zlib.gzipSync(fs.readFileSync(path.join(webRoot, 'mcbench-pack.bin')), { level: 9 }));
  files.set('mesher.wasm.gz', zlib.gzipSync(fs.readFileSync(mesher), { level: 9 }));
  for (const name of CODE) files.set(name, fs.readFileSync(path.join(WEB, name)));
  return files;
}

// Writes siteDir/viewer/<viewerId>/ unless it is there, and returns its id:
// a hash of every path and byte in it, so a changed bake or app code is a
// new folder and the pages of earlier shares keep the viewer they were made with.
function writeViewer(siteDir, { webRoot, mesher }) {
  const files = viewerFiles(webRoot, mesher);
  const hash = crypto.createHash('sha256');
  for (const [name, bytes] of [...files].sort(([a], [b]) => (a < b ? -1 : 1))) {
    hash.update(`${name}\n${crypto.createHash('sha256').update(bytes).digest('hex')}\n`);
  }
  const viewerId = hash.digest('hex').slice(0, 16);
  const folder = path.join(siteDir, 'viewer', viewerId);
  if (fs.existsSync(folder)) return viewerId;
  const staging = `${folder}.${process.pid}.partial`;
  fs.rmSync(staging, { recursive: true, force: true });
  for (const [name, bytes] of files) {
    fs.mkdirSync(path.dirname(path.join(staging, name)), { recursive: true });
    fs.writeFileSync(path.join(staging, name), bytes);
  }
  fs.renameSync(staging, folder);
  return viewerId;
}

function escapeHtml(text) {
  return String(text).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

// The page of a share, b/<shareId>/index.html. Its tags are what X's
// crawler (which runs no JavaScript) and other link previews read: X shows
// `url` itself, this page, in a 480x480 player frame. The page is the viewer
// filling the window, under a cover until the first build is on screen (the
// share bridge tells it), with a line along the bottom while the rest is
// drawn, or saying why it could not be shown. `labels` name the share's
// builds in order; with more than one, a strip of them along the top
// switches the viewer between them (the left and right arrow keys too).
// The labels are in the markup only, as escaped pill text, which the
// script reads back. Bottom left, the camera's mode (Fly: BlueMap's free
// flight; Orbit: its map controls) and a button back to the opening view;
// a hint of the mode's controls on this device shows when the build first
// appears and when the mode changes, and fades.
function sharePageHtml({ viewerId, shareId, title, description, url, image, labels }) {
  const t = escapeHtml(title);
  const d = escapeHtml(description);
  const u = escapeHtml(url);
  const i = escapeHtml(image);
  const strip = labels.length > 1 ? `<nav id="builds" aria-label="Builds"><div><button type="button" id="prev" aria-label="Previous build">&lsaquo;</button><div id="pills">${labels.map((label, index) => `<button type="button" class="pill" data-index="${index}" aria-pressed="${index === 0}">${escapeHtml(label)}</button>`).join('')}</div><button type="button" id="next" aria-label="Next build">&rsaquo;</button></div></nav>
` : '';
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no">
<title>${t}</title>
<meta name="description" content="${d}">
<meta name="twitter:card" content="player">
<meta name="twitter:title" content="${t}">
<meta name="twitter:description" content="${d}">
<meta name="twitter:image" content="${i}">
<meta name="twitter:player" content="${u}">
<meta name="twitter:player:width" content="480">
<meta name="twitter:player:height" content="480">
<meta property="og:title" content="${t}">
<meta property="og:description" content="${d}">
<meta property="og:image" content="${i}">
<meta property="og:url" content="${u}">
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
<iframe id="viewer" src="../../viewer/${escapeHtml(viewerId)}/?build=../../b/${escapeHtml(shareId)}/build.json" title="${t}" allow="fullscreen"></iframe>
<div id="cover" role="status">Loading the build…</div>
<div id="progress" role="progressbar" hidden><div></div><span></span></div>
<a id="full" href="./" target="_blank" rel="noopener">Open full screen</a>
<div id="view"><button type="button" data-mode="fly" aria-pressed="false" title="Fly: move freely through the build">Fly</button><button type="button" data-mode="orbit" aria-pressed="false" title="Orbit: turn around the build">Orbit</button><button type="button" id="reset" title="Reset view" aria-label="Reset view"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 12a9 9 0 1 0 3-6.7"/><path d="M3 4v5h5"/></svg></button></div>
<div id="hint" role="note"></div>
${strip}<script>
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
`;
}

module.exports = { writeViewer, sharePageHtml };
