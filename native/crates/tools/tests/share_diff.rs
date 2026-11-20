//! The share differential's Rust side and its comparison: the chat and the
//! bench `tests/node/share.js` published with today's `server/share.js`,
//! published again with [`mcbench_tools::share::Shares`] over the same
//! worlds (`<out>/worlds/<id>.blueprint`), the same given `gh`, viewer and
//! export composition (`worldBlueprint` at the manifest's height,
//! `renderMask`, the bounds lifted, a named stand-in for the shot), and the
//! same fixed version; then every file of each `b/<shareId>/`, the viewer's
//! file and `.nojekyll` compared byte for byte, gzip members after
//! decompression, with the page's address as each side's Pages gives it
//! (today's fetch is a fake, this side's wait reads a local server); and
//! the commits pushed and the listing.
//!
//! `MCBENCH_SHARE_DIFF=<out of share.js>`; ignored without it.

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mcbench_render::Cell;
use mcbench_tools::share::{
    BenchSource, Bounds, Export, Exported, Failure, Options, Processes, Ran, Run, SharePage,
    Shares, SiteViewer, Source, Subject, share_id,
};
use mcbench_world::blueprint::{blueprint_world, world_blueprint};
use mcbench_world::live_views::render_mask;
use serde_json::{Value, json};

const NODE_BASE: &str = "https://owner.github.io/builds/";

fn git_env(out: &Path) -> Vec<(String, String)> {
    [
        (
            "GIT_CONFIG_GLOBAL",
            out.join("gitconfig").display().to_string(),
        ),
        ("GIT_CONFIG_NOSYSTEM", "1".into()),
        ("GIT_AUTHOR_NAME", "Test".into()),
        ("GIT_AUTHOR_EMAIL", "test@example.invalid".into()),
        ("GIT_COMMITTER_NAME", "Test".into()),
        ("GIT_COMMITTER_EMAIL", "test@example.invalid".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

fn git(out: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .envs(git_env(out))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8(output.stdout).unwrap()
}

struct Given {
    out: PathBuf,
    port: u16,
}

#[async_trait]
impl Run for Given {
    async fn run(
        &self,
        command: &str,
        args: &[String],
        cwd: Option<&Path>,
        env: &[(String, String)],
    ) -> Result<Ran, Failure> {
        if command == "gh" && args[1] == "user" {
            return Ok(Ran {
                stdout: "owner\n42+owner@users.noreply.github.com\n".into(),
                stderr: String::new(),
            });
        }
        if command == "gh" {
            return Ok(Ran {
                stdout: json!({ "html_url": format!("http://127.0.0.1:{}/builds/", self.port), "build_type": "legacy", "source": { "branch": "main", "path": "/" } }).to_string(),
                stderr: String::new(),
            });
        }
        let mut all = git_env(&self.out);
        all.extend(env.iter().cloned());
        Processes.run(command, args, cwd, &all).await
    }
}

struct Composed {
    y_offset: i32,
}

#[async_trait]
impl Export for Composed {
    async fn export_build(&self, source: &Source, size: [u32; 2]) -> Result<Exported, String> {
        let bounds = source.world.bounds().expect("a build");
        let y = self.y_offset;
        let lift = |[x, by, z]: [i32; 3]| [x, by + y, z];
        Ok(Exported {
            blueprint: world_blueprint(&source.world, y)?,
            mask: render_mask(Some(bounds)),
            bounds: Bounds {
                min: lift(bounds.min),
                max: lift(bounds.max),
            },
            preview: format!("shot of {} {}x{}", source.model, size[0], size[1]).into_bytes(),
        })
    }
    async fn montage(
        &self,
        tiles: Vec<Vec<u8>>,
        cells: Vec<Cell>,
        size: [u32; 2],
    ) -> Result<Vec<u8>, String> {
        let tiles: Vec<String> = tiles
            .into_iter()
            .map(|tile| String::from_utf8(tile).unwrap())
            .collect();
        Ok(mcbench_json::stringify(
            &json!({ "tiles": tiles, "cells": serde_json::to_value(&cells).unwrap(), "size": size }),
        )
        .into_bytes())
    }
}

struct FixedViewer;

impl SiteViewer for FixedViewer {
    fn write_viewer(&self, site: &Path) -> Result<String, String> {
        std::fs::create_dir_all(site.join("viewer").join("v1")).unwrap();
        std::fs::write(site.join("viewer").join("v1").join("viewer.js"), "viewer").unwrap();
        Ok("v1".into())
    }
    fn share_page_html(&self, page: &SharePage<'_>) -> String {
        format!(
            "<html>{}</html>",
            mcbench_json::stringify(&json!({
                "viewerId": page.viewer_id,
                "shareId": page.share_id,
                "title": page.title,
                "description": page.description,
                "url": page.url,
                "image": page.image,
                "labels": page.labels,
            }))
        )
    }
}

/// Pages: every build.json read answers the fixed version.
fn pages(version: String) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buffer).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..n]);
            }
            let body = json!({ "version": version }).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    port
}

fn source(out: &Path, value: &Value) -> Source {
    let id = value["id"].as_str().unwrap().to_string();
    let bytes = std::fs::read(out.join("worlds").join(format!("{id}.blueprint"))).unwrap();
    Source {
        title: value["title"].as_str().unwrap().into(),
        model: value["model"].as_str().unwrap().into(),
        effort: value["effort"].as_str().map(String::from),
        world: blueprint_world(&bytes, 0, None).unwrap(),
        id,
    }
}

fn files(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .unwrap();
    out
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs MCBENCH_SHARE_DIFF=<out of tests/node/share.js>"]
async fn shares_write_the_files_todays_share_js_writes() {
    let out = PathBuf::from(std::env::var("MCBENCH_SHARE_DIFF").expect("MCBENCH_SHARE_DIFF"));
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap();
    let node: Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("node.json")).unwrap()).unwrap();
    let version = manifest["version"].as_str().unwrap().to_string();
    let fixed = version.clone();
    mcbench_tools::verify::set_uuid_source(Box::new(move || fixed.clone())).unwrap();

    let bare = out.join("rust-remote.git");
    let b = bare.to_str().unwrap();
    git(&out, &["init", "--quiet", "--bare", "-b", "main", b]);
    let seed = format!("{b}-seed");
    git(&out, &["clone", "--quiet", b, &seed]);
    std::fs::write(Path::new(&seed).join("README.md"), "shared builds\n").unwrap();
    git(&out, &["-C", &seed, "add", "-A"]);
    git(&out, &["-C", &seed, "commit", "--quiet", "-m", "init"]);
    git(
        &out,
        &["-C", &seed, "push", "--quiet", "origin", "HEAD:main"],
    );

    let port = pages(version);
    let remote = b.to_string();
    let shares = Shares::new(Options {
        root: out.join("rust"),
        remote: Box::new(move |_| remote.clone()),
        run: Arc::new(Given {
            out: out.clone(),
            port,
        }),
        export: Arc::new(Composed {
            y_offset: manifest["yOffset"].as_i64().unwrap() as i32,
        }),
        viewer: Arc::new(FixedViewer),
        wait: Duration::from_secs(60),
        poll: Duration::from_millis(5),
        network: Duration::from_secs(120),
        local: Duration::from_secs(30),
    });
    let chat = source(&out, &manifest["chat"]);
    let members: Vec<Source> = manifest["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| source(&out, member))
        .collect();
    let bench = &manifest["bench"];
    let bench = BenchSource {
        id: bench["id"].as_str().unwrap().into(),
        title: bench["title"].as_str().unwrap().into(),
        blind: bench["blind"].as_bool().unwrap(),
        revealed: bench["revealed"].as_bool().unwrap(),
        order: bench["order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().to_string())
            .collect(),
    };
    let ids = [share_id(&chat.id), share_id(&bench.id)];
    let rust_base = format!("http://127.0.0.1:{port}/builds/");
    let mut rust_states = Vec::new();
    for subject in [Subject::chat(chat), Subject::Bench { bench, members }] {
        let states = Arc::new(std::sync::Mutex::new(Vec::new()));
        let noted = Arc::clone(&states);
        shares
            .publish("owner/builds", subject, move |step| {
                noted.lock().unwrap().push(step.to_json())
            })
            .unwrap()
            .await;
        let states = states.lock().unwrap().clone();
        rust_states.push(mcbench_json::stringify(&Value::Array(states)));
    }
    let (node_site, rust_site) = (out.join("node").join("site"), out.join("rust").join("site"));
    let mut compared = Vec::new();
    let mut differ = Vec::new();
    // Text is compared with this side's Pages address as today's; a
    // blueprint (not UTF-8) as it is.
    let mut check = |name: String, node: Vec<u8>, rust: Vec<u8>| {
        let rust = match String::from_utf8(rust) {
            Ok(text) => text.replace(&rust_base, NODE_BASE).into_bytes(),
            Err(binary) => binary.into_bytes(),
        };
        compared.push(format!("{name} ({} bytes)", node.len()));
        if node != rust {
            differ.push(name);
        }
    };
    for id in &ids {
        let (node, rust) = (node_site.join("b").join(id), rust_site.join("b").join(id));
        assert_eq!(files(&node), files(&rust), "b/{id}/");
        for file in files(&node) {
            let (a, b) = (
                std::fs::read(node.join(&file)).unwrap(),
                std::fs::read(rust.join(&file)).unwrap(),
            );
            if file.ends_with(".gz") {
                check(format!("b/{id}/{file} (inflated)"), gunzip(&a), gunzip(&b));
            } else {
                check(format!("b/{id}/{file}"), a, b);
            }
        }
    }
    for file in [".nojekyll", "viewer/v1/viewer.js"] {
        check(
            file.into(),
            std::fs::read(node_site.join(file)).unwrap(),
            std::fs::read(rust_site.join(file)).unwrap(),
        );
    }
    let commits = git(
        &out,
        &[
            "--git-dir",
            b,
            "log",
            "--format=%an <%ae> %cn <%ce> %s",
            "main",
        ],
    );
    let commits: Vec<&str> = commits.trim().split('\n').collect();
    let node_commits: Vec<&str> = node["commits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    check(
        "commits".into(),
        node_commits.join("\n").into_bytes(),
        commits.join("\n").into_bytes(),
    );
    let listed: Vec<Value> = shares
        .list("owner/builds")
        .await
        .unwrap()
        .into_iter()
        .map(|build| json!({ "shareId": build.share_id, "title": build.title, "kind": build.kind, "labels": build.labels, "url": build.url }))
        .collect();
    check(
        "list".into(),
        mcbench_json::stringify(&node["list"]).into_bytes(),
        mcbench_json::stringify(&Value::Array(listed)).into_bytes(),
    );
    let node_states: Vec<String> = node["shares"]
        .as_array()
        .unwrap()
        .iter()
        .map(|share| mcbench_json::stringify(&share["states"]))
        .collect();
    check(
        "publish moves".into(),
        node_states.join("\n").into_bytes(),
        rust_states.join("\n").into_bytes(),
    );
    eprintln!(
        "share diff: compared {}:\n  {}",
        compared.len(),
        compared.join("\n  ")
    );
    assert!(differ.is_empty(), "differ: {differ:?}");
}
