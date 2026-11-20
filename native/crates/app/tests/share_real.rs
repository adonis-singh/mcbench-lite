//! A publish through the app's own adapter, end to end: a core started as
//! the app starts it (its own backend, setup, engines and renderer, the
//! hardware adapter with no window) on a scratch data folder that already
//! holds the prepared data and textures, so setup fetches nothing; the
//! real `AppSite` as the export (`share::RenderedExport`: the settle and
//! the preview shot) and the viewer (`share_viewer`); git for real against
//! a local bare repository; gh a stand-in that names a Pages site on a
//! local listener, which serves the bare repository's `main` and BlueMap's
//! CLI jar. Setup finds what it would fetch already made, so nothing
//! leaves the machine.
//!
//! Ignored by default: it needs, named by the environment,
//! `MCBENCH_SHARE_PREPARED` (a folder holding `minecraft/<id>/` and
//! `textures/<id>/` as setup leaves them), `MCBENCH_SHARE_JAR` (BlueMap
//! 5.22's CLI jar), `MCBENCH_BUILD_CHILD` (the build child's program) and a
//! graphics adapter.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use flate2::read::GzDecoder;
use serde_json::json;
use sha2::{Digest, Sha256};

use mcbench_lite::core::live::LiveParts;
use mcbench_lite::core::share::Stand;
use mcbench_lite::core::{Core, Parts};
use mcbench_tools::share::{self, Failure, Options, Processes, Ran, Run, share_id};
use mcbench_ui::{Command, PublishStep, ShareRead, Subject, Update, Window};

const WAIT: Duration = Duration::from_secs(300);
const REPO: &str = "owner/builds";

#[derive(Default)]
struct Recording {
    updates: Mutex<Vec<Update>>,
    arrived: Condvar,
}

impl Window for Recording {
    fn publish(&self, update: Update) {
        self.updates.lock().unwrap().push(update);
        self.arrived.notify_all();
    }
}

impl Recording {
    fn wait<T>(&self, from: usize, what: &str, find: impl Fn(&[Update]) -> Option<T>) -> T {
        let updates = self.updates.lock().unwrap();
        let (updates, timeout) = self
            .arrived
            .wait_timeout_while(updates, WAIT, |u| find(&u[from..]).is_none())
            .unwrap();
        assert!(!timeout.timed_out(), "{what}");
        find(&updates[from..]).unwrap()
    }

    fn len(&self) -> usize {
        self.updates.lock().unwrap().len()
    }
}

fn git(args: &[&str]) -> Vec<u8> {
    let out = std::process::Command::new("git")
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// git as the app runs it (no settings of the machine's), gh a stand-in.
struct GitAndPages {
    pages: String,
}

#[async_trait]
impl Run for GitAndPages {
    async fn run(
        &self,
        command: &str,
        args: &[String],
        cwd: Option<&Path>,
        env: &[(String, String)],
    ) -> Result<Ran, Failure> {
        let ran = |stdout: String| {
            Ok(Ran {
                stdout,
                stderr: String::new(),
            })
        };
        match (command, args[1].as_str()) {
            ("gh", "user") => ran("owner\n42+owner@users.noreply.github.com\n".into()),
            ("gh", pages) if pages.ends_with("/pages") => ran(json!({
                "html_url": self.pages,
                "build_type": "legacy",
                "source": { "branch": "main", "path": "/" },
            })
            .to_string()),
            ("git", _) => {
                let mut all: Vec<(String, String)> = vec![
                    ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
                    ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
                ];
                all.extend(env.iter().cloned());
                Processes.run(command, args, cwd, &all).await
            }
            _ => panic!("no {command} {args:?} here"),
        }
    }
}

/// The Pages site and the jar's host: `/bluemap.jar` is the jar, any other
/// path the file of the bare repository's `main`.
fn serve(bare: PathBuf, jar: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" || header.is_empty() {
                    break;
                }
            }
            let path = line.split(' ').nth(1).unwrap().split('?').next().unwrap();
            let body = if path == "/bluemap.jar" {
                Some(jar.clone())
            } else {
                let out = std::process::Command::new("git")
                    .args(["--git-dir"])
                    .arg(&bare)
                    .args(["show", &format!("main:{}", path.trim_start_matches('/'))])
                    .stderr(Stdio::null())
                    .output()
                    .unwrap();
                out.status.success().then_some(out.stdout)
            };
            let response = match body {
                Some(body) => {
                    let mut r = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    r.extend(body);
                    r
                }
                None => b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    .to_vec(),
            };
            let _ = stream.write_all(&response);
        }
    });
    address
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn tree(bare: &Path, prefix: &str) -> Vec<String> {
    String::from_utf8(git(&[
        "--git-dir",
        bare.to_str().unwrap(),
        "ls-tree",
        "-r",
        "--name-only",
        "main",
        prefix,
    ]))
    .unwrap()
    .lines()
    .map(String::from)
    .collect()
}

fn show(bare: &Path, file: &str) -> Vec<u8> {
    git(&[
        "--git-dir",
        bare.to_str().unwrap(),
        "show",
        &format!("main:{file}"),
    ])
}

fn props(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
#[ignore = "needs prepared data and textures, BlueMap's jar, the build child and a graphics adapter"]
fn a_chat_is_published_through_the_real_export_and_viewer_needs_prepared_data_and_a_graphics_adapter()
 {
    let prepared = PathBuf::from(std::env::var("MCBENCH_SHARE_PREPARED").unwrap());
    let jar = std::fs::read(std::env::var("MCBENCH_SHARE_JAR").unwrap()).unwrap();
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("share-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let data = root.join("data");
    copy_tree(&prepared, &data);
    let id = std::fs::read_dir(data.join("minecraft"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name();
    std::fs::write(
        data.join("settings.json"),
        json!({ "shareRepo": REPO }).to_string(),
    )
    .unwrap();

    // The remote: a bare repository with one commit on main.
    let bare = root.join("remote.git");
    let seed = root.join("seed");
    git(&[
        "init",
        "--quiet",
        "--bare",
        "-b",
        "main",
        bare.to_str().unwrap(),
    ]);
    git(&[
        "clone",
        "--quiet",
        bare.to_str().unwrap(),
        seed.to_str().unwrap(),
    ]);
    std::fs::write(seed.join("README.md"), "shared builds\n").unwrap();
    let s = seed.to_str().unwrap();
    git(&["-C", s, "add", "-A"]);
    git(&["-C", s, "commit", "--quiet", "-m", "init"]);
    git(&["-C", s, "push", "--quiet", "origin", "HEAD:main"]);

    let sha256 = Sha256::digest(&jar)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let bytes = jar.len() as u64;
    let site = serve(bare.clone(), jar);
    // The app's client: setup finds its data and textures made and fetches
    // nothing; the jar's address is the loopback's.
    let client = mcbench_setup::fetch::client();
    let folder = data.join("minecraft").join(&id);
    let (remote, pages) = (bare.to_str().unwrap().to_string(), site.clone());
    let share_root = data.join("share");
    let core = Core::start(Parts {
        data: data.clone(),
        program: PathBuf::from(env!("CARGO_BIN_EXE_mcbench-lite")),
        host: Arc::new(mcbench_store::SystemHost),
        shipped: Box::new(move || Ok(mcbench_setup::Data::at(&folder))),
        client,
        backend: None,
        installer: None,
        share: Some(Stand {
            jar: mcbench_setup::pins::Download {
                url: format!("{site}bluemap.jar"),
                sha256,
                bytes,
            },
            options: Box::new(move |app| Options {
                root: share_root,
                remote: Box::new(move |_| remote.clone()),
                run: Arc::new(GitAndPages { pages }),
                export: app.clone(),
                viewer: app,
                wait: share::WAIT,
                poll: Duration::from_millis(100),
                network: share::NETWORK,
                local: share::LOCAL,
            }),
        }),
        live: LiveParts::default(),
    })
    .expect("the core starts");
    let window = Arc::new(Recording::default());
    mcbench_ui::Core::attach(&core, window.clone());
    window
        .wait(0, "the renderer's start", |u| {
            u.iter().find_map(|u| match u {
                Update::LivePane(pane) => Some(pane.as_ref().map(drop).map_err(Clone::clone)),
                _ => None,
            })
        })
        .expect("the renderer starts");

    // The chat: a floor of stone and planks, glass, and a lever switched on
    // wired to a lamp stored unlit: the settle lights it.
    let handle = core.handle().clone();
    let on = core.clone();
    let chat = handle
        .block_on(handle.spawn(async move {
            on.create_conversation("openrouter", "chat/model")
                .await
                .unwrap()
                .id
        }))
        .unwrap();
    {
        let live = core.store().conversation(&chat).unwrap().unwrap();
        let mut conversation = live.lock().unwrap();
        let world = &mut conversation.world;
        for x in 0..6 {
            for z in 0..6 {
                let block = if (x + z) % 2 == 0 {
                    "minecraft:stone"
                } else {
                    "minecraft:oak_planks"
                };
                world.set_block([x, 0, z], block, &[]).unwrap();
            }
        }
        world.set_block([5, 1, 5], "minecraft:glass", &[]).unwrap();
        let lever = props(&[("face", "floor"), ("facing", "north"), ("powered", "true")]);
        world
            .set_block([1, 1, 1], "minecraft:lever", &lever)
            .unwrap();
        world
            .set_block([2, 1, 1], "minecraft:redstone_wire", &[])
            .unwrap();
        let lamp = props(&[("lit", "false")]);
        world
            .set_block([3, 1, 1], "minecraft:redstone_lamp", &lamp)
            .unwrap();
        world.finalize();
        core.store().save(&mut conversation).unwrap();
    }
    let subject = Subject::Chat(chat.clone());
    let command = |command: Command| {
        let from = window.len();
        let on = core.clone();
        handle
            .block_on(handle.spawn(async move { on.command(command).await }))
            .unwrap();
        from
    };

    // The publish: its four moves, in order.
    let from = command(Command::Publish {
        subject: subject.clone(),
    });
    let steps = window.wait(from, "the publish's end", |u| {
        let steps: Vec<PublishStep> = u
            .iter()
            .filter_map(|u| match u {
                Update::PublishStep { subject: s, step } if *s == subject => Some(step.clone()),
                _ => None,
            })
            .collect();
        steps
            .iter()
            .any(|s| matches!(s, PublishStep::Live { .. } | PublishStep::Failed { .. }))
            .then_some(steps)
    });
    let shared = share_id(&chat);
    let url = format!("{site}b/{shared}/");
    assert_eq!(
        steps,
        [
            PublishStep::Exporting,
            PublishStep::Publishing,
            PublishStep::Waiting,
            PublishStep::Live { url: url.clone() }
        ]
    );

    // The pushed site.
    let viewer = tree(&bare, "viewer/");
    let viewer_id = viewer[0].split('/').nth(1).unwrap().to_string();
    assert!(
        viewer
            .iter()
            .all(|p| p.starts_with(&format!("viewer/{viewer_id}/")))
    );
    println!("viewer {viewer_id}: {} paths", viewer.len());
    assert_eq!(viewer.len(), 53, "{viewer:#?}");
    let mut page = tree(&bare, &format!("b/{shared}/"));
    page.sort();
    let named = |n: &str| format!("b/{shared}/{n}");
    assert_eq!(
        page,
        [
            named("0.bin.gz"),
            named("build.json"),
            named("index.html"),
            named("preview.jpg")
        ]
    );
    let html = String::from_utf8(show(&bare, &named("index.html"))).unwrap();
    assert!(html.contains(&format!("viewer/{viewer_id}/")) || html.contains(&viewer_id));

    // The preview: a JPEG of 1024 by 1024 (`exportBuild`'s chat size), drawn.
    let jpeg = show(&bare, &named("preview.jpg"));
    assert_eq!(&jpeg[..3], [0xff, 0xd8, 0xff], "a JPEG's start");
    let picture = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)
        .unwrap()
        .to_rgb8();
    assert_eq!(picture.dimensions(), (1024, 1024));
    let colours: std::collections::HashSet<[u8; 3]> = picture.pixels().map(|p| p.0).collect();
    println!(
        "preview.jpg: {} bytes, {}x{}, {} distinct colours",
        jpeg.len(),
        picture.width(),
        picture.height(),
        colours.len()
    );
    assert!(colours.len() > 1000, "{} colours", colours.len());
    std::fs::write(root.join("preview.jpg"), &jpeg).unwrap();

    // The blueprint: the world as Minecraft settles it, lifted to its
    // height. The lamp, stored unlit, is lit; the dust carries power.
    let mut blueprint = Vec::new();
    GzDecoder::new(&show(&bare, &named("0.bin.gz"))[..])
        .read_to_end(&mut blueprint)
        .unwrap();
    let mut blocks = Vec::new();
    mcbench_world::blueprint::for_each_blueprint_block(&blueprint, |x, y, z, entry| {
        blocks.push(([x, y, z], entry.0.clone(), entry.1.clone()));
    })
    .unwrap();
    println!("blueprint: {} blocks", blocks.len());
    assert_eq!(blocks.len(), 36 + 4);
    let at = |name: &str| {
        blocks
            .iter()
            .find(|(_, n, _)| n == name)
            .unwrap_or_else(|| panic!("no {name} in {blocks:?}"))
            .clone()
    };
    let (lamp_at, _, lamp) = at("minecraft:redstone_lamp");
    println!("lamp at {lamp_at:?}: {lamp:?}");
    assert!(lamp.contains(&("lit".into(), "true".into())), "{lamp:?}");
    let (_, _, dust) = at("minecraft:redstone_wire");
    println!("dust: {dust:?}");
    assert!(
        dust.iter().any(|(k, v)| k == "power" && v != "0"),
        "{dust:?}"
    );
    assert_eq!(lamp_at[1], 65, "lifted to Minecraft's height");
    let build: serde_json::Value =
        serde_json::from_slice(&show(&bare, &named("build.json"))).unwrap();
    assert_eq!(build["url"], url.as_str());
    assert_eq!(build["builds"][0]["label"], "chat/model");

    // Listed, read, removed.
    let from = command(Command::ReadShares);
    let listed = window.wait(from, "the list", |u| {
        u.iter().find_map(|u| match u {
            Update::Shares(listed) => Some(listed.clone()),
            _ => None,
        })
    });
    let listed = listed.unwrap();
    assert_eq!(listed.repo.as_deref(), Some(REPO));
    assert!(
        listed.builds.iter().any(|b| b.share_id == shared
            && b.url == url
            && b.conversation_id.as_deref() == Some(&chat)),
        "{listed:?}"
    );
    let from = command(Command::ReadShare {
        subject: subject.clone(),
    });
    let read = window.wait(from, "the read", |u| {
        u.iter().find_map(|u| match u {
            Update::ShareRead { read, .. } => Some(read.clone()),
            _ => None,
        })
    });
    assert_eq!(
        read,
        Ok(ShareRead {
            repo: Some(REPO.into()),
            url: Some(url.clone())
        })
    );
    let head = git(&["--git-dir", bare.to_str().unwrap(), "rev-parse", "main"]);
    let from = command(Command::Unshare {
        subject: subject.clone(),
    });
    let changed = window.wait(from, "the removal", |u| {
        u.iter().find_map(|u| match u {
            Update::ShareChanged { url, .. } => Some(url.clone()),
            Update::Error(e) => panic!("{e}"),
            _ => None,
        })
    });
    assert_eq!(changed, None);
    let parent = git(&["--git-dir", bare.to_str().unwrap(), "rev-parse", "main^"]);
    assert_eq!(parent, head, "the removal is the next commit");
    assert!(tree(&bare, &format!("b/{shared}/")).is_empty());
    assert_eq!(tree(&bare, "viewer/").len(), 53, "the viewer stays");

    core.shutdown();
}
