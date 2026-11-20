//! The texture fetch from a jar served on loopback: every entry checked
//! against the manifest, nothing kept that fails, ranges resumed.

mod common;

use std::collections::HashMap;
use std::io::Read;

use common::{Answer, names, scratch, serve, sha256};
use mcbench_setup::fetch::{Options, fetch, ranges};
use mcbench_setup::jar::{Entry, Manifest, entries, write_zip};
use tokio_util::sync::CancellationToken;

/// A jar of `count` entries of 3 KiB each, every other one deflated, with
/// `gap` stored bytes of another entry between them, and its manifest of
/// the wanted ones.
fn make_jar(count: usize, gap: usize) -> (Vec<u8>, Manifest) {
    let mut files: Vec<(Entry, Vec<u8>)> = Vec::new();
    for i in 0..count {
        let bytes: Vec<u8> = (0..3072u32)
            .map(|j| ((j * 7 + i as u32 * 13) % 251) as u8)
            .collect();
        let (method, stored) = if i % 2 == 0 {
            let mut out = Vec::new();
            flate2::read::DeflateEncoder::new(&bytes[..], flate2::Compression::default())
                .read_to_end(&mut out)
                .unwrap();
            (8, out)
        } else {
            (0, bytes.clone())
        };
        let e = Entry {
            name: format!("assets/minecraft/textures/block/t{i}.png"),
            offset: 0,
            data: 0,
            stored: stored.len() as u64,
            size: bytes.len() as u64,
            method,
            crc32: crc32fast::hash(&bytes),
            sha256: sha256(&bytes),
        };
        files.push((e, stored));
        let filler = vec![0u8; gap];
        let f = Entry {
            name: format!("filler/{i}"),
            offset: 0,
            data: 0,
            stored: gap as u64,
            size: gap as u64,
            method: 0,
            crc32: crc32fast::hash(&filler),
            sha256: sha256(&filler),
        };
        files.push((f, filler));
    }
    let list: Vec<(&Entry, &[u8])> = files.iter().map(|(e, s)| (e, s.as_slice())).collect();
    let jar = write_zip(&list);
    let placed = entries(&jar).unwrap();
    let wanted = files
        .iter()
        .zip(&placed)
        .filter(|((e, _), _)| !e.name.starts_with("filler/"))
        .map(|((e, _), p)| Entry {
            offset: p.offset,
            data: p.data,
            ..e.clone()
        })
        .collect();
    let manifest = Manifest {
        minecraft: "1.21.10".into(),
        url: String::new(),
        size: jar.len() as u64,
        sha1: String::new(),
        entries: wanted,
        materials: vec![],
        entity_atlas: vec![],
        particles: vec![],
    };
    (jar, manifest)
}

fn options(url: &str) -> Options {
    Options {
        url: url.into(),
        gap: 16 * 1024,
        connections: 4,
        rate: None,
    }
}

#[tokio::test]
async fn every_entry_is_fetched_in_coalesced_ranges_and_checked() {
    let (jar, manifest) = make_jar(12, 40_000);
    let server = serve(HashMap::from([("/client.jar".to_string(), jar)])).await;
    let dir = scratch("fetch-all");
    let got = fetch(
        &common::client(),
        &manifest,
        &options(&format!("{}/client.jar", server.url)),
        &dir,
        &|_| {},
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(got.bytes.len(), 12);
    assert_eq!(got.requests, ranges(&manifest.entries, 16 * 1024).len());
    assert_eq!(got.requests, 12, "entries 40 KB apart are not coalesced");
    for e in &manifest.entries {
        assert_eq!(sha256(&got.bytes[&e.name]), e.sha256);
    }
    let close = make_jar(12, 1000).1;
    assert_eq!(
        ranges(&close.entries, 16 * 1024).len(),
        1,
        "entries 1 KB apart are one range"
    );
    assert!(
        server
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|(_, range)| range.is_some())
    );
}

#[tokio::test]
async fn a_range_whose_bytes_are_not_the_manifests_fails_and_is_not_kept() {
    let (mut jar, manifest) = make_jar(4, 40_000);
    let at = manifest.entries[2].data as usize + 10;
    jar[at] ^= 0xff;
    let server = serve(HashMap::from([("/client.jar".to_string(), jar)])).await;
    let dir = scratch("fetch-tampered");
    let error = fetch(
        &common::client(),
        &manifest,
        &options(&format!("{}/client.jar", server.url)),
        &dir,
        &|_| {},
        &CancellationToken::new(),
    )
    .await
    .err()
    .unwrap();
    assert!(error.starts_with(&manifest.entries[2].name), "{error}");
    let kept = names(&dir);
    let bad = ranges(&manifest.entries, 16 * 1024)
        .into_iter()
        .find(|r| r.entries.iter().any(|e| e.name == manifest.entries[2].name))
        .unwrap();
    assert!(
        !kept.contains(&format!("{}-{}", bad.start, bad.end)),
        "{kept:?}"
    );
    assert!(kept.iter().all(|n| !n.ends_with(".part")));
}

#[tokio::test]
async fn a_whole_jar_where_a_range_was_asked_is_refused() {
    let (jar, manifest) = make_jar(2, 40_000);
    let server = serve(HashMap::from([("/client.jar".to_string(), jar)])).await;
    server.state.lock().unwrap().script = vec![Answer::Status(200), Answer::Status(200)];
    let error = fetch(
        &common::client(),
        &manifest,
        &options(&format!("{}/client.jar", server.url)),
        &scratch("fetch-200"),
        &|_| {},
        &CancellationToken::new(),
    )
    .await
    .err()
    .unwrap();
    assert!(error.contains("HTTP 200"), "{error}");
}

#[tokio::test]
async fn a_fetch_that_stopped_resumes_with_the_ranges_it_lacks() {
    let (jar, manifest) = make_jar(6, 40_000);
    let server = serve(HashMap::from([("/client.jar".to_string(), jar)])).await;
    // One range at a time: the first two are answered, the third is not.
    server.state.lock().unwrap().script = vec![Answer::Serve, Answer::Serve, Answer::Status(503)];
    let dir = scratch("fetch-resume");
    let mut one = options(&format!("{}/client.jar", server.url));
    one.connections = 1;
    assert!(
        fetch(
            &common::client(),
            &manifest,
            &one,
            &dir,
            &|_| {},
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert_eq!(names(&dir).len(), 2, "the two checked ranges are kept");
    let asked = server.state.lock().unwrap().requests.len();
    let got = fetch(
        &common::client(),
        &manifest,
        &one,
        &dir,
        &|_| {},
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(got.bytes.len(), 6);
    assert_eq!(got.requests, 4);
    assert_eq!(server.state.lock().unwrap().requests.len() - asked, 4);
}
