//! `test/share-viewer.test.js` ported, by name (12 of 12), and the
//! differential of [`mcbench_tools::share_viewer`] against today's
//! `writeViewer` and `sharePageHtml` and the bake's web app.
//!
//! The six tests whose subject is the bridge's or the mesher worker's
//! JavaScript run today's test body in Node (`tests/node/share-viewer.js`)
//! against the files a viewer folder written by [`write_viewer`] holds:
//! they need `MCBENCH_SHARE_NODE=<a node_modules folder>` (Node on PATH),
//! are ignored without it and fail when it is set and Node fails. The
//! differentials need `MCBENCH_DIFF_DATA=<the clean fixture>` too.
//! Every download comes from a server on loopback.

#[path = "../../agent/tests/common/fixture.rs"]
mod fixture;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use mcbench_setup::jar::{self, Entry};
use mcbench_setup::pins::{self, Download};
use mcbench_tools::share_viewer::{
    CODE, MESHER, ViewerParts, bluemap_web_app, share_page_html, viewer_files, web_app_folder,
    write_viewer,
};
use regex::Regex;
use sha2::Digest;
use tokio_util::sync::CancellationToken;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The page's code as the crate embeds it (`share-page/`).
fn web(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("share-page")
            .join(name),
    )
    .unwrap()
}

/// The crate's copies are the localhost app's files, byte for byte, for as
/// long as `web/` is in the tree (a check's tree has `native/` without it,
/// and the localhost app goes at stage five; then the copies are the files).
#[test]
fn the_embedded_page_code_equals_the_localhost_apps_while_it_is_in_the_tree() {
    let localhost = repo().join("web");
    if !localhost.is_dir() {
        return;
    }
    for (name, embedded) in mcbench_tools::share_viewer::CODE {
        assert_eq!(
            std::fs::read(localhost.join(name)).unwrap(),
            embedded,
            "share-page/{name} against web/{name}"
        );
    }
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .unwrap();
    out
}

fn sha256(bytes: &[u8]) -> String {
    jar::hex(&sha2::Sha256::digest(bytes))
}

fn write_all(root: &Path, files: &[(&str, &[u8])]) {
    for (name, bytes) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}

/// A baked BlueMap web folder as McbenchBake writes it, with stand-in
/// bytes, and the viewer's parts read from it.
fn baked_web(root: &Path) -> ViewerParts {
    let web = root.join("web");
    write_all(
        &web,
        &[
            ("index.html", br#"<!DOCTYPE html><script type="module" src="./assets/index-1.js"></script>"#),
            ("assets/index-1.js", br#"console.log("bluemap")"#),
            ("assets/index-1.js.map", b"{}"),
            ("assets/index-1.css", b"body{}"),
            ("lang/settings.conf", b"{}"),
            ("lang/en.conf", b"{}"),
            ("settings.json", br#"{"version":"5.22","useCookies":true,"maps":["live"],"scripts":["/bluemap-bridge.js"],"styles":[]}"#),
            ("maps/live/settings.json", br#"{"name":"Live build","skyColor":[0.49,0.67,1,1],"voidColor":[0,0,0,1]}"#),
            ("maps/live/textures.json", b"[]"),
            ("maps/live/live/markers.json", b"{}"),
            ("maps/live/live/players.json", b"{}"),
            ("mcbench-pack.bin", b"the pack"),
            ("mcbench-entities.json", b"{}"),
            ("mcbench-entities.png", b"png"),
            ("sql.php", b"<?php"),
        ],
    );
    parts_of(&web)
}

fn parts_of(web: &Path) -> ViewerParts {
    ViewerParts {
        web_app: web.to_path_buf(),
        textures: web.join("maps/live/textures.json"),
        pack: web.join("mcbench-pack.bin"),
        settings: web.join("settings.json"),
        map_settings: web.join("maps/live/settings.json"),
    }
}

fn list_files(folder: &Path) -> Vec<String> {
    fn walk(root: &Path, relative: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(root.join(relative)).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let name = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            if entry.file_type().unwrap().is_dir() {
                walk(root, &name, out);
            } else {
                out.push(name);
            }
        }
    }
    let mut out = Vec::new();
    walk(folder, "", &mut out);
    out.sort();
    out
}

fn names(folder: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

fn json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn the_viewer_folder_holds_bluemaps_web_app_the_map_the_gzipped_pack_and_mesher_the_apps_mesher_modules_and_the_share_bridge_and_nothing_else()
 {
    let root = tempfile::tempdir().unwrap();
    let parts = baked_web(root.path());
    let site = root.path().join("site");
    let id = write_viewer(&site, &parts).unwrap();
    assert!(Regex::new("^[0-9a-f]{16}$").unwrap().is_match(&id));
    let folder = site.join("viewer").join(&id);
    assert_eq!(
        list_files(&folder),
        [
            "assets/index-1.css",
            "assets/index-1.js",
            "bluemap-tiles.mjs",
            "bluemap-view.mjs",
            "index.html",
            "lang/en.conf",
            "lang/settings.conf",
            "maps/live/live/markers.json",
            "maps/live/live/players.json",
            "maps/live/settings.json",
            "maps/live/textures.json",
            "mcbench-pack.bin.gz",
            "mesher-worker.mjs",
            "mesher.mjs",
            "mesher.wasm.gz",
            "settings.json",
            "share/bridge.js",
        ]
    );
    assert_eq!(
        names(&site.join("viewer")),
        std::slice::from_ref(&id),
        "no staging folder is left"
    );
    // The app's own code, byte for byte.
    for name in [
        "bluemap-tiles.mjs",
        "bluemap-view.mjs",
        "mesher-worker.mjs",
        "mesher.mjs",
        "share/bridge.js",
    ] {
        assert_eq!(
            std::fs::read(folder.join(name)).unwrap(),
            web(name),
            "{name}"
        );
    }
    assert_eq!(
        gunzip(&std::fs::read(folder.join("mcbench-pack.bin.gz")).unwrap()),
        b"the pack"
    );
    assert_eq!(
        gunzip(&std::fs::read(folder.join("mesher.wasm.gz")).unwrap()),
        std::fs::read(repo().join("renderer/mcbench-renderer.wasm")).unwrap()
    );
    // BlueMap loads the share bridge at an address relative to the viewer, and
    // keeps no camera or settings in cookies between shares.
    let settings = json(&folder.join("settings.json"));
    assert_eq!(settings["scripts"], serde_json::json!(["share/bridge.js"]));
    assert_eq!(settings["useCookies"], false);
    // No tile is asked for before the bridge runs: the site has no tile files.
    assert_eq!(
        [
            &settings["hiresSliderDefault"],
            &settings["lowresSliderDefault"],
            &settings["lowresSliderMin"]
        ],
        [
            &serde_json::json!(0),
            &serde_json::json!(0),
            &serde_json::json!(0)
        ]
    );
    assert_eq!(settings["maps"], serde_json::json!(["live"]));
    // Sky, not black void, below the horizon.
    assert_eq!(
        json(&folder.join("maps/live/settings.json")),
        serde_json::json!({"name": "Live build", "skyColor": [0.49, 0.67, 1, 1], "voidColor": [0.49, 0.67, 1, 1]})
    );

    // The same content is the same folder, left as it is.
    let stamp = std::fs::metadata(folder.join("settings.json"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(write_viewer(&site, &parts).unwrap(), id);
    assert_eq!(
        std::fs::metadata(folder.join("settings.json"))
            .unwrap()
            .modified()
            .unwrap(),
        stamp
    );
    // Other content is another folder.
    std::fs::write(&parts.pack, "another pack").unwrap();
    let other = write_viewer(&site, &parts).unwrap();
    assert_ne!(other, id);
    let mut both = vec![id, other];
    both.sort();
    assert_eq!(names(&site.join("viewer")), both);
}

#[test]
fn the_share_bridge_and_the_modules_it_loads_use_only_addresses_relative_to_the_viewer() {
    // GitHub Pages serves the site under /<repo>/: an address from the root misses it.
    let rooted = Regex::new(r#"(import\(|from |new Worker\(|fetch\()\s*['"`]/"#).unwrap();
    for (name, bytes) in CODE {
        assert_eq!(bytes, web(name), "{name} is web/'s, as embedded");
        assert!(
            !rooted.is_match(std::str::from_utf8(bytes).unwrap()),
            "{name}"
        );
    }
    assert_eq!(
        MESHER,
        std::fs::read(repo().join("renderer/mcbench-renderer.wasm")).unwrap()
    );
}

struct Page {
    viewer_id: &'static str,
    share_id: &'static str,
    title: &'static str,
    description: &'static str,
    url: &'static str,
    image: &'static str,
}

const PAGE: Page = Page {
    viewer_id: "0123456789abcdef",
    share_id: "k3y",
    title: r#"A <castle> & "keep""#,
    description: "Built by model-x in 12 turns'",
    url: "https://someone.github.io/builds/b/k3y/",
    image: "https://someone.github.io/builds/b/k3y/preview.png",
};

fn page(labels: &[&str]) -> String {
    let labels: Vec<String> = labels.iter().map(|l| l.to_string()).collect();
    share_page_html(
        PAGE.viewer_id,
        PAGE.share_id,
        PAGE.title,
        PAGE.description,
        PAGE.url,
        PAGE.image,
        &labels,
    )
}

/// The content of the `<meta>` with this name or property, as a crawler that
/// runs no JavaScript reads it.
fn meta(html: &str, key: &str) -> Option<String> {
    Regex::new(&format!(
        r#"<meta (?:name|property)="{}" content="([^"]*)">"#,
        regex::escape(key)
    ))
    .unwrap()
    .captures(html)
    .map(|c| c[1].to_string())
}

fn matches(html: &str, pattern: &str) -> bool {
    Regex::new(pattern).unwrap().is_match(html)
}

fn script(html: &str) -> &str {
    &html[html.find("<script>").unwrap()..html.find("</script>").unwrap()]
}

#[test]
fn the_share_page_carries_xs_player_card_and_open_graph_tags_in_its_static_markup_escaped() {
    let html = page(&["model-x"]);
    assert!(matches(&html, "(?i)^<!doctype html>"));
    let title = "A &lt;castle&gt; &amp; &quot;keep&quot;";
    let description = "Built by model-x in 12 turns&#39;";
    let m = |key| meta(&html, key);
    assert_eq!(m("twitter:card").as_deref(), Some("player"));
    assert_eq!(m("twitter:title").as_deref(), Some(title));
    assert_eq!(m("twitter:description").as_deref(), Some(description));
    assert_eq!(m("twitter:player").as_deref(), Some(PAGE.url));
    assert_eq!(m("twitter:player:width").as_deref(), Some("480"));
    assert_eq!(m("twitter:player:height").as_deref(), Some("480"));
    assert_eq!(m("twitter:image").as_deref(), Some(PAGE.image));
    assert_eq!(m("og:title").as_deref(), Some(title));
    assert_eq!(m("og:description").as_deref(), Some(description));
    assert_eq!(m("og:image").as_deref(), Some(PAGE.image));
    assert_eq!(m("og:url").as_deref(), Some(PAGE.url));
    assert_eq!(m("og:type").as_deref(), Some("website"));
    assert!(html.contains(&format!("<title>{title}</title>")));
    assert!(!html.contains("<castle>"));
    // The viewer, at an address relative to the page, told where the build is.
    assert!(matches(
        &html,
        r#"<iframe id="viewer" src="\.\./\.\./viewer/0123456789abcdef/\?build=\.\./\.\./b/k3y/build\.json""#
    ));
    // A link that opens the page itself in a new tab.
    assert!(
        html.contains(
            r#"<a id="full" href="./" target="_blank" rel="noopener">Open full screen</a>"#
        )
    );
    // A progress line, hidden until the bridge tells how far drawing has come.
    assert!(html.contains(r#"<div id="progress" role="progressbar" hidden>"#));
    // X's sandbox has no modals and no forms.
    assert!(!matches(&html, r"alert\(|confirm\(|prompt\(|<form"));
    // A share of one build has no strip of builds, and has the camera's
    // controls and the hint.
    assert!(!matches(
        &html,
        r#"id="builds"|class="pill"|id="prev"|id="next""#
    ));
    assert!(matches(
        &html,
        r#"<div id="view"><button type="button" data-mode="fly" aria-pressed="false"[^>]*>Fly</button><button type="button" data-mode="orbit" aria-pressed="false"[^>]*>Orbit</button><button type="button" id="reset" title="Reset view" aria-label="Reset view">"#
    ));
    assert!(html.contains(r#"<div id="hint" role="note"></div>"#));
}

const LABELS: [&str; 3] = [
    "claude-opus-5-5-20251101",
    "<img src=x onerror=alert(1)>",
    "</script><script>alert(2)</script>",
];

#[test]
fn a_share_of_several_builds_has_a_strip_of_them_along_the_top_in_order_the_first_pressed_every_label_escaped()
 {
    let html = page(&LABELS);
    let pills: Vec<[String; 3]> = Regex::new(
        r#"<button type="button" class="pill" data-index="(\d+)" aria-pressed="(true|false)">([^<]*)</button>"#,
    )
    .unwrap()
    .captures_iter(&html)
    .map(|c| [c[1].to_string(), c[2].to_string(), c[3].to_string()])
    .collect();
    assert_eq!(
        pills,
        [
            ["0", "true", "claude-opus-5-5-20251101"],
            ["1", "false", "&lt;img src=x onerror=alert(1)&gt;"],
            [
                "2",
                "false",
                "&lt;/script&gt;&lt;script&gt;alert(2)&lt;/script&gt;"
            ],
        ]
        .map(|p| p.map(String::from))
    );
    assert!(html.contains(
        r#"<nav id="builds" aria-label="Builds"><div><button type="button" id="prev" aria-label="Previous build">"#
    ));
    assert!(html.contains(r#"<button type="button" id="next" aria-label="Next build">"#));
    // No label is written raw anywhere: not as markup and not in the script,
    // which has exactly one <script> and one </script>.
    assert!(!html.contains("<img"));
    assert_eq!(html.matches("<script>").count(), 1);
    assert_eq!(html.matches("</script>").count(), 1);
    assert!(
        html.find(r#"<nav id="builds""#).unwrap() < html.find("<script>").unwrap(),
        "the strip is markup before the script"
    );
    for label in LABELS {
        assert!(
            !script(&html).contains(label),
            "{label} is not in the script"
        );
    }
    // The tags crawlers read are those of a share of one build.
    let one = page(&["model-x"]);
    for key in [
        "twitter:card",
        "twitter:title",
        "twitter:description",
        "twitter:image",
        "twitter:player",
        "twitter:player:width",
        "twitter:player:height",
        "og:title",
        "og:description",
        "og:image",
        "og:url",
        "og:type",
        "description",
    ] {
        assert_eq!(meta(&html, key), meta(&one, key), "{key}");
    }
    let title = Regex::new("<title>[^<]*</title>").unwrap();
    assert_eq!(
        title.find(&html).unwrap().as_str(),
        title.find(&one).unwrap().as_str()
    );
    // The page and a share of one build differ only by the strip.
    assert_eq!(
        Regex::new(r#"<nav id="builds"[^\n]*</nav>\n"#)
            .unwrap()
            .replace(&html, ""),
        one
    );
}

#[test]
fn the_share_page_switches_the_cameras_mode_and_resets_the_view_through_the_bridge_and_follows_the_mode_the_bridge_tells()
 {
    let html = page(&LABELS);
    let script = script(&html);
    assert!(script.contains(
        "postMessage({ type: 'mcbench-share:set-mode', mode: button.dataset.mode }, window.location.origin)"
    ));
    assert!(
        script.contains("postMessage({ type: 'mcbench-share:reset' }, window.location.origin)")
    );
    assert!(script.contains("'mcbench-share:mode') return markMode(event.data.mode)"));
    assert!(script.contains("'mcbench-share:input') return hideHint()"));
    // The hint is written as text, for the device the bridge chose the mode by.
    assert!(script.contains("hint.textContent = HINTS[mode]"));
    assert!(script.contains("matchMedia('(pointer: coarse) and (hover: none)')"));
    // The controls sit bottom left, clear of the progress label and the full-screen link.
    assert!(html.contains("#view{position:fixed;left:8px;bottom:8px;"));
    assert!(html.contains("#progress>span{position:absolute;left:136px;bottom:8px;"));
}

#[test]
fn the_share_page_asks_the_bridge_for_a_build_by_index_and_hears_only_of_the_build_it_asked_for_from_the_viewer_on_its_origin()
 {
    let html = page(&LABELS);
    let script = script(&html);
    assert!(script.contains(
        "postMessage({ type: 'mcbench-share:show', index: index }, window.location.origin)"
    ));
    assert!(script.contains(
        "event.source !== frame.contentWindow || event.origin !== window.location.origin"
    ));
    assert!(script.contains("event.data.index !== wanted"));
    assert!(script.contains("'the viewer did not load.'); }, 60000)"));
    // The strip scrolls itself, never the page around it.
    assert!(!script.contains("scrollIntoView"));
}

fn node_modules() -> String {
    std::env::var("MCBENCH_SHARE_NODE").expect("MCBENCH_SHARE_NODE=<a node_modules folder>")
}

fn node(args: &[&str]) -> String {
    let modules = node_modules();
    assert!(Path::new(&modules).is_dir(), "{modules} is not a folder");
    let output = mcbench_spawn::command("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/node/share-viewer.js"))
        .args(args)
        .env("NODE_PATH", &modules)
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "node {args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Today's test `name`, run in Node against a viewer folder this crate wrote.
fn node_case(name: &str) {
    let root = tempfile::tempdir().unwrap();
    let parts = baked_web(root.path());
    let site = root.path().join("site");
    let id = write_viewer(&site, &parts).unwrap();
    let folder = site.join("viewer").join(id);
    assert_eq!(node(&["case", name, folder.to_str().unwrap()]).trim(), "ok");
}

#[test]
#[ignore = "needs MCBENCH_SHARE_NODE=<node_modules> and Node"]
fn the_worker_reads_an_address_ending_in_gz_as_gzip_as_it_streams_and_the_same_world_comes_of_it() {
    node_case(
        "the worker reads an address ending in .gz as gzip, as it streams, and the same world comes of it",
    );
}

#[test]
#[ignore = "needs MCBENCH_SHARE_NODE=<node_modules> and Node"]
fn the_opening_view_sees_the_builds_box_from_the_south_east_30_degrees_down_whole_and_centered_at_its_middle_height()
 {
    node_case(
        "the opening view sees the build's box from the south-east, 30 degrees down, whole and centered, at its middle height",
    );
}

#[test]
#[ignore = "needs MCBENCH_SHARE_NODE=<node_modules> and Node"]
fn a_build_shown_after_another_is_fitted_from_the_direction_the_visitor_looks_whole_and_centered_at_its_middle_height()
 {
    node_case(
        "a build shown after another is fitted from the direction the visitor looks: whole and centered at its middle height",
    );
}

#[test]
#[ignore = "needs MCBENCH_SHARE_NODE=<node_modules> and Node"]
fn the_share_bridge_loads_only_a_build_in_its_own_sites_b_folder_and_throws_for_any_other_address()
{
    node_case(
        "the share bridge loads only a build in its own site's b/ folder, and throws for any other address",
    );
}

#[test]
#[ignore = "needs MCBENCH_SHARE_NODE=<node_modules> and Node"]
fn fly_places_the_camera_at_the_orbit_views_eye_at_distance_0_looking_the_same_way_the_same_picture()
 {
    node_case(
        "fly places the camera at the orbit view's eye, at distance 0, looking the same way: the same picture",
    );
}

#[test]
#[ignore = "needs MCBENCH_SHARE_NODE=<node_modules> and Node"]
fn a_share_opens_in_free_flight_and_in_orbit_on_a_device_whose_only_pointer_is_a_finger() {
    node_case(
        "a share opens in free flight, and in orbit on a device whose only pointer is a finger",
    );
}

// BlueMap's web app from its jar.

/// A server on loopback answering every request with `body` (or 404 when
/// it is `None`), counting the requests.
async fn serve(body: Option<Vec<u8>>) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            counted.fetch_add(1, Ordering::SeqCst);
            let body = body.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut buffer).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&buffer[..n]);
                }
                let (status, body) = match body {
                    Some(body) => ("200 OK", body),
                    None => ("404 Not Found", Vec::new()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(head.as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            });
        }
    });
    (format!("http://{address}/bluemap-5.22-cli.jar"), count)
}

fn deflated_entry(name: &str, bytes: &[u8]) -> (Entry, Vec<u8>) {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    let stored = encoder.finish().unwrap();
    (
        Entry {
            name: name.into(),
            offset: 0,
            data: 0,
            stored: stored.len() as u64,
            size: bytes.len() as u64,
            method: 8,
            crc32: crc32fast::hash(bytes),
            sha256: String::new(),
        },
        stored,
    )
}

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let entries: Vec<(Entry, Vec<u8>)> = files
        .iter()
        .map(|(name, bytes)| deflated_entry(name, bytes))
        .collect();
    let refs: Vec<(&Entry, &[u8])> = entries.iter().map(|(e, s)| (e, s.as_slice())).collect();
    jar::write_zip(&refs)
}

/// A stand-in for BlueMap's CLI jar: a class file and the web app's zip.
fn stand_in_jar() -> Vec<u8> {
    let web_app = zip(&[
        ("index.html", b"<!DOCTYPE html>"),
        ("assets/", b""),
        ("assets/index-1.js", b"console.log(\"bluemap\")"),
        ("lang/en.conf", b"{}"),
    ]);
    zip(&[
        ("de/bluecolored/bluemap/BlueMap.class", b"class"),
        ("de/bluecolored/bluemap/webapp.zip", &web_app),
    ])
}

#[tokio::test]
async fn bluemaps_web_app_is_read_from_its_jar_downloaded_once_and_kept_in_the_data_folder() {
    let jar_bytes = stand_in_jar();
    let (url, requests) = serve(Some(jar_bytes.clone())).await;
    let data = tempfile::tempdir().unwrap();
    let pin = Download {
        url,
        sha256: sha256(&jar_bytes),
        bytes: jar_bytes.len() as u64,
    };
    let client = mcbench_setup::fetch::client();
    let cancel = CancellationToken::new();
    let folder = bluemap_web_app(data.path(), &client, &pin, &cancel)
        .await
        .unwrap();
    assert_eq!(folder, web_app_folder(data.path()));
    assert_eq!(
        list_files(&folder),
        ["assets/index-1.js", "index.html", "lang/en.conf"]
    );
    assert_eq!(
        std::fs::read(folder.join("assets/index-1.js")).unwrap(),
        b"console.log(\"bluemap\")"
    );
    // The jar is kept beside it, and nothing else is left.
    assert_eq!(
        names(&data.path().join("bluemap")),
        [
            format!("bluemap-{}-cli.jar", pins::BLUEMAP_VERSION),
            format!("webapp-{}", pins::BLUEMAP_VERSION)
        ]
    );
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    // A second call fetches nothing.
    assert_eq!(
        bluemap_web_app(data.path(), &client, &pin, &cancel)
            .await
            .unwrap(),
        folder
    );
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    // A viewer is made of it.
    let bake = tempfile::tempdir().unwrap();
    let mut parts = baked_web(bake.path());
    parts.web_app = folder;
    let files = viewer_files(&parts).unwrap();
    assert_eq!(files["index.html"], b"<!DOCTYPE html>");
    assert!(!files.contains_key("assets/index-1.css"));
}

#[tokio::test]
async fn a_jar_whose_sha256_differs_or_that_is_not_there_is_not_kept_and_the_failure_says_to_try_again()
 {
    let jar_bytes = stand_in_jar();
    let client = mcbench_setup::fetch::client();
    let cancel = CancellationToken::new();
    for (body, cause) in [
        (Some(jar_bytes.clone()), "but mcbench-lite expects"),
        (None, "(HTTP 404)"),
    ] {
        let (url, _) = serve(body).await;
        let data = tempfile::tempdir().unwrap();
        let pin = Download {
            url,
            sha256: "0".repeat(64),
            bytes: jar_bytes.len() as u64,
        };
        let error = bluemap_web_app(data.path(), &client, &pin, &cancel)
            .await
            .unwrap_err();
        assert!(
            error.starts_with(&format!(
                "BlueMap {} could not be downloaded from GitHub for the shared page (",
                pins::BLUEMAP_VERSION
            )),
            "{error}"
        );
        assert!(error.contains(cause), "{error}");
        assert!(error.ends_with("). Try again."), "{error}");
        assert_eq!(names(&data.path().join("bluemap")), Vec::<String>::new());
    }
}

// The differentials, on the clean fixture.

fn fixture_bake() -> PathBuf {
    let data = PathBuf::from(
        std::env::var("MCBENCH_DIFF_DATA").expect("MCBENCH_DIFF_DATA=<the clean fixture>"),
    );
    fixture::guard(&data);
    let web = data.join("minecraft-live/bluemap/8d91567eef72f773/web");
    assert!(web.is_dir(), "{} is not a folder", web.display());
    web
}

fn read_tree(folder: &Path) -> BTreeMap<String, Vec<u8>> {
    list_files(folder)
        .into_iter()
        .map(|name| {
            let bytes = std::fs::read(folder.join(&name)).unwrap();
            (name, bytes)
        })
        .collect()
}

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA=<the clean fixture>, MCBENCH_SHARE_NODE=<node_modules> and Node"]
fn the_viewer_folder_and_the_share_pages_are_todays_for_the_bakes_inputs() {
    let bake = fixture_bake();
    let out = tempfile::tempdir().unwrap();
    let node_id = node(&["diff", bake.to_str().unwrap(), out.path().to_str().unwrap()])
        .trim()
        .to_string();
    let node_files = read_tree(&out.path().join("node-site/viewer").join(&node_id));
    let site = out.path().join("rust-site");
    let id = write_viewer(&site, &parts_of(&bake)).unwrap();
    let ours = read_tree(&site.join("viewer").join(&id));
    assert_eq!(
        ours.keys().collect::<Vec<_>>(),
        node_files.keys().collect::<Vec<_>>(),
        "the same paths"
    );
    let mut same_bytes = 0;
    let mut same_inflated = 0;
    for (name, bytes) in &ours {
        let theirs = &node_files[name];
        if bytes == theirs {
            same_bytes += 1;
        } else {
            assert!(name.ends_with(".gz"), "{name} differs");
            assert_eq!(gunzip(bytes), gunzip(theirs), "{name} inflated");
            same_inflated += 1;
        }
    }
    // The ids differ only by the gzip members' bytes: with Node's members
    // in place of ours, the hash is Node's id.
    let mut swapped = ours.clone();
    for (name, bytes) in swapped.iter_mut() {
        if name.ends_with(".gz") {
            *bytes = node_files[name].clone();
        }
    }
    let swapped_id = mcbench_tools::share_viewer::viewer_id(&swapped);
    println!(
        "viewer: {} paths, {same_bytes} identical, {same_inflated} gzip members identical inflated; ids {id} (ours) {node_id} (Node), equal: {}; with Node's gzip members: {swapped_id}",
        ours.len(),
        id == node_id
    );
    assert_eq!(swapped_id, node_id);

    let pages = json(&out.path().join("pages.json"));
    let mut equal = 0;
    for row in pages.as_array().unwrap() {
        let p = &row["page"];
        let s = |key: &str| p[key].as_str().unwrap();
        let labels: Vec<String> = p["labels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap().to_string())
            .collect();
        let html = share_page_html(
            s("viewerId"),
            s("shareId"),
            s("title"),
            s("description"),
            s("url"),
            s("image"),
            &labels,
        );
        assert_eq!(html, row["html"].as_str().unwrap(), "{p}");
        equal += 1;
    }
    println!("pages: {equal} of 3 identical");
    assert_eq!(equal, 3);
}

#[tokio::test]
#[ignore = "needs MCBENCH_DIFF_DATA=<the clean fixture>"]
async fn bluemaps_web_app_from_the_pinned_jar_is_the_bakes() {
    let bake = fixture_bake();
    let jar_file = bake.join("../../bluemap-5.22-cli.jar");
    let jar_bytes = std::fs::read(&jar_file).unwrap();
    let (url, requests) = serve(Some(jar_bytes)).await;
    let pin = Download {
        url,
        ..pins::bluemap()
    };
    let data = tempfile::tempdir().unwrap();
    let folder = bluemap_web_app(
        data.path(),
        &mcbench_setup::fetch::client(),
        &pin,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    let ours = read_tree(&folder);
    let baked = read_tree(&bake);
    let mut same = 0;
    for (name, bytes) in &ours {
        if let Some(theirs) = baked.get(name) {
            assert_eq!(sha256(bytes), sha256(theirs), "{name}");
            same += 1;
        }
    }
    let only_ours: Vec<&String> = ours.keys().filter(|k| !baked.contains_key(*k)).collect();
    let only_bake: Vec<&String> = baked.keys().filter(|k| !ours.contains_key(*k)).collect();
    println!(
        "web app: {} files written, {same} equal to the bake's by sha256; only ours: {only_ours:?}; only the bake's: {only_bake:?}",
        ours.len()
    );
    assert_eq!(same + only_ours.len(), ours.len());
}
