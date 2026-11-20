// The in-process renderer (mcbench_draw::live::Draw) answers as today's
// server and helper answer, sentence for sentence, where see_build's answer
// to a model is made of them:
//
//   a shot the helper cannot read      serve.rs take: `shots: <serde's reason>`
//   a renderer that cannot start       serve.rs hello: Gpu::open's sentence as it is
//   a request that stalls              server/shot.js stage: `Screenshot <step> stalled
//                                      for <s>s; renderer reset, screenshot unavailable`,
//                                      then a fresh renderer holding no world
//   a second lost device               the next request on a renderer made from nothing,
//                                      as the server's fresh helper
//
// The first is compared against the helper itself (mcbench-draw serve, built
// from this crate). The tests that draw need Minecraft's baked data
// (MCBENCH_DRAW_DATA with a night.json beside it, as tests/live.rs) and are
// skipped without it; the start's needs none.

mod common;

use mcbench_draw::assets::Paths;
use mcbench_draw::gpu::Choice;
use mcbench_draw::live::{pane_stalled, Draw, STAGE};
use mcbench_draw::serve::{read_frame, write_frame};
use mcbench_render::live::{LiveRenderer, PaneStatus};
use mcbench_render::{Camera, PictureFormat, RenderError, Renderer, Shot, WorldUpload};
use serde_json::{json, Value};
use std::io::{BufReader, BufWriter, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

// One request to a fresh helper after its hello: the replies' headers.
fn helper(hello: Value, then: Option<Value>) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcbench-draw"))
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = BufWriter::new(child.stdin.take().unwrap());
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut replies = Vec::new();
    for header in std::iter::once(hello).chain(then) {
        write_frame(&mut input, header, &[]).unwrap();
        input.flush().unwrap();
        replies.push(read_frame(&mut output).unwrap().expect("a reply").header);
    }
    drop(input);
    child.wait().unwrap();
    replies
}

fn hello(paths: &Paths, software: bool) -> Value {
    let mut header =
        json!({ "id": 1, "type": "hello", "assets": serde_json::to_value(paths).unwrap() });
    if software {
        header["adapter"] = json!("software");
    }
    header
}

#[test]
fn a_renderer_that_cannot_start_answers_the_helpers_hello_sentence() {
    // The software floor without its driver: on macOS there is none
    // ("No software adapter on macOS: Metal or nothing"), on Linux the build
    // was given no SwiftShader; Windows has WARP, so there the assets that
    // are not there fail the start instead. Nothing on the GPU is touched
    // on macOS and Linux.
    let missing = common::crate_dir().join("tests/no-such-assets");
    let paths = mcbench_draw::synthetic::paths(&missing);
    let replies = helper(hello(&paths, true), None);
    let helper_says = replies[0]["error"]
        .as_str()
        .expect("hello fails")
        .to_string();
    let choice = Choice {
        software: true,
        software_vulkan: None,
    };
    let Err(RenderError::NoAdapter(draw_says)) = Draw::open(paths, choice, 1 << 30, STAGE) else {
        panic!("the start fails as a start");
    };
    eprintln!("the helper: {helper_says}\nin process: {draw_says}");
    assert_eq!(draw_says, helper_says);
    if cfg!(target_os = "macos") {
        assert_eq!(draw_says, "No software adapter on macOS: Metal or nothing");
    }
}

struct Setup {
    draw: Draw,
    paths: Paths,
}

fn setup(test: &str, bound: Duration) -> Option<Setup> {
    let data = common::data(test)?;
    let mut paths = Paths::of_data(&data).unwrap();
    // The software floor's driver, as the server names it (hello's
    // softwareVulkan) and the Draw is given it.
    paths.software_vulkan = common::software_vulkan();
    let draw = Draw::open(paths.clone(), common::choice(), 1 << 30, bound).unwrap();
    Some(Setup { draw, paths })
}

fn upload(draw: &Draw) -> Result<mcbench_render::WorldReport, RenderError> {
    let blueprint = common::blueprint(&[
        ("minecraft:stone", [0, 64, 0]),
        ("minecraft:stone", [1, 64, 0]),
        ("minecraft:gold_block", [0, 65, 0]),
    ]);
    draw.world(WorldUpload {
        name: "w".into(),
        y_offset: 64,
        blueprint,
        entities: None,
        base: None,
    })
}

fn shot() -> Shot {
    serde_json::from_str(r#"{"width":64,"height":48,"camera":{"yaw":-0.785,"pitch":0.524,"focus":[0.5,64.5,0.5],"focusDist":6,"fov":70},"clip":null,"grid":null}"#).unwrap()
}

fn shots(draw: &Draw) -> Result<Vec<u8>, RenderError> {
    let never = AtomicBool::new(false);
    let mut pictures = draw.shots("w", &[shot()], 2, PictureFormat::Png, &never)?;
    Ok(pictures.remove(0).bytes)
}

// A shot a view named after Object.prototype's `constructor` gives
// (server/shot.js:367-370: VIEWS.constructor is Object, whose yaw and pitch
// are undefined: the camera's numbers are NaN, which JSON.stringify writes
// as null). The focus is the box's centre, a number.
fn nan_shot() -> Shot {
    Shot {
        camera: Camera {
            yaw: f64::NAN,
            pitch: f64::NAN,
            focus_dist: f64::NAN,
            ..shot().camera
        },
        ..shot()
    }
}

#[test]
fn a_shot_the_helper_cannot_read_answers_the_helpers_sentence() {
    let Some(s) = setup(
        "a_shot_the_helper_cannot_read_answers_the_helpers_sentence",
        STAGE,
    ) else {
        return;
    };
    // The helper given the same shot as the server sends it: its JSON.
    let sent = serde_json::to_value(nan_shot()).unwrap();
    assert_eq!(sent["camera"]["yaw"], Value::Null);
    let replies = helper(
        hello(&s.paths, common::software()),
        Some(
            json!({ "id": 2, "type": "shots", "world": "w", "shots": [sent], "supersample": 2, "format": { "type": "jpeg", "quality": 90 } }),
        ),
    );
    assert_eq!(replies[0]["ok"], true, "{}", replies[0]);
    let helper_says = replies[1]["error"].as_str().expect("refused").to_string();
    upload(&s.draw).unwrap();
    let never = AtomicBool::new(false);
    let Err(RenderError::Failed(draw_says)) =
        s.draw
            .shots("w", &[nan_shot()], 2, PictureFormat::Jpeg(90), &never)
    else {
        panic!("the shot is refused");
    };
    eprintln!("the helper: {helper_says}\nin process: {draw_says}");
    assert_eq!(draw_says, helper_says);
    assert_eq!(draw_says, "shots: invalid type: null, expected f64");
}

#[test]
fn a_stalled_request_answers_todays_sentence_and_the_next_starts_afresh() {
    // Long enough that the requests before and after the stall, which it
    // bounds too, never reach it on a loaded machine.
    let bound = Duration::from_secs(5);
    let Some(s) = setup(
        "a_stalled_request_answers_todays_sentence_and_the_next_starts_afresh",
        bound,
    ) else {
        return;
    };
    let pane = s.draw.live();
    upload(&s.draw).unwrap();
    let before = shots(&s.draw).unwrap();
    // A shots call on a render thread that does not answer.
    s.draw.hold(Duration::from_secs(20));
    let t = Instant::now();
    let Err(RenderError::Stalled(sentence)) = shots(&s.draw) else {
        panic!("the call stalls");
    };
    let waited = t.elapsed();
    assert_eq!(
        sentence,
        "Screenshot capture stalled for 5s; renderer reset, screenshot unavailable"
    );
    assert!(
        waited >= bound && waited < Duration::from_secs(15),
        "{waited:?}"
    );
    assert_eq!(pane.status(), PaneStatus::Failed(pane_stalled(bound)));
    // The next request on a fresh renderer, which holds no world: the world
    // is sent again (as the server's retry sends it to a fresh helper) and
    // draws the same picture.
    let Err(RenderError::UnknownWorld(e)) = shots(&s.draw) else {
        panic!("a fresh renderer holds no world");
    };
    assert_eq!(e, "No world named w is kept");
    upload(&s.draw).unwrap();
    assert!(shots(&s.draw).unwrap() == before, "the same picture afresh");
    // A world upload stalls under its own step's name; the pane stays as it
    // was stopped.
    s.draw.hold(Duration::from_secs(20));
    let Err(RenderError::Stalled(sentence)) = upload(&s.draw) else {
        panic!("the upload stalls");
    };
    assert_eq!(
        sentence,
        "Screenshot world loading stalled for 5s; renderer reset, screenshot unavailable"
    );
    upload(&s.draw).unwrap();
    assert!(shots(&s.draw).unwrap() == before);
    assert_eq!(pane.status(), PaneStatus::Failed(pane_stalled(bound)));
    s.draw.shutdown();
    assert!(!s.draw.running());
}

#[test]
fn a_second_lost_device_is_answered_by_a_renderer_made_from_nothing() {
    let Some(s) = setup(
        "a_second_lost_device_is_answered_by_a_renderer_made_from_nothing",
        STAGE,
    ) else {
        return;
    };
    let pane = s.draw.live();
    upload(&s.draw).unwrap();
    let before = shots(&s.draw).unwrap();
    // The first loss: made again in place, the world kept.
    s.draw.lose_device();
    let deadline = Instant::now() + Duration::from_secs(10);
    while s.draw.stats().recreated == 0 {
        assert!(Instant::now() < deadline, "the device made again");
        s.draw.accounting();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(shots(&s.draw).unwrap() == before);
    // The second: the live view stops with its sentence.
    s.draw.lose_device();
    let deadline = Instant::now() + Duration::from_secs(10);
    while pane.status() == PaneStatus::Drawing {
        assert!(Instant::now() < deadline, "the second loss stops the pane");
        s.draw.accounting();
        std::thread::sleep(Duration::from_millis(10));
    }
    let PaneStatus::Failed(stopped) = pane.status() else {
        unreachable!()
    };
    assert!(
        stopped.starts_with("The graphics device was lost: "),
        "{stopped}"
    );
    // The next request: a renderer made again from nothing, holding no
    // world, as the server's fresh helper; the world sent again draws the
    // same picture.
    let Err(RenderError::UnknownWorld(e)) = shots(&s.draw) else {
        panic!("a renderer made from nothing holds no world");
    };
    assert_eq!(e, "No world named w is kept");
    upload(&s.draw).unwrap();
    assert!(shots(&s.draw).unwrap() == before, "the same picture afresh");
    assert_eq!(pane.status(), PaneStatus::Failed(stopped));
}
