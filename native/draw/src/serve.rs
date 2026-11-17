// `mcbench-draw serve`: PROTOCOL.md's frames on stdin and stdout. A reader
// thread takes frames and answers `cancel` at once; one worker answers the
// rest in order. JPEG encoding runs on rayon's threads, so the next shot
// draws while one encodes.

use crate::assets::{Assets, Paths};
use crate::gpu::{Choice, Failure, Gpu, Renderer};
use crate::picture::{self, Font};
use crate::store::{self, Kept, Shot, Store};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

pub const MAX_HEADER: usize = 16 << 20;
pub const MAX_PAYLOADS: usize = 64;
pub const MAX_PAYLOAD: u64 = 2 << 30;

pub struct Frame {
    pub header: Value,
    pub payloads: Vec<Vec<u8>>,
}

// Reads one frame; None at the end of the stream before a frame.
pub fn read_frame(input: &mut impl Read) -> Result<Option<Frame>, String> {
    let mut length = [0u8; 4];
    match input.read_exact(&mut length) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(format!("Reading a frame: {e}")),
    }
    let n = u32::from_le_bytes(length) as usize;
    if n > MAX_HEADER {
        return Err(format!("A frame's header of {n} bytes passes the limit of {MAX_HEADER}"));
    }
    let mut text = vec![0u8; n];
    input.read_exact(&mut text).map_err(|e| format!("Reading a frame's header: {e}"))?;
    let header: Value = serde_json::from_slice(&text).map_err(|e| format!("A frame's header is not JSON: {e}"))?;
    if !header.is_object() {
        return Err("A frame's header is not a JSON object".into());
    }
    let lengths: Vec<u64> = match header.get("payloads") {
        None => Vec::new(),
        Some(list) => list.as_array().and_then(|a| a.iter().map(Value::as_u64).collect()).ok_or("A frame's payloads are not a list of byte lengths")?,
    };
    if lengths.len() > MAX_PAYLOADS || lengths.iter().any(|&l| l > MAX_PAYLOAD) {
        return Err("A frame's payloads pass the limits".into());
    }
    let mut payloads = Vec::with_capacity(lengths.len());
    for l in lengths {
        let mut p = vec![0u8; l as usize];
        input.read_exact(&mut p).map_err(|e| format!("Reading a frame's payload: {e}"))?;
        payloads.push(p);
    }
    Ok(Some(Frame { header, payloads }))
}

pub fn write_frame(out: &mut impl Write, mut header: Value, payloads: &[&[u8]]) -> std::io::Result<()> {
    if !payloads.is_empty() {
        header["payloads"] = json!(payloads.iter().map(|p| p.len()).collect::<Vec<_>>());
    }
    let text = serde_json::to_vec(&header).expect("a reply serializes");
    out.write_all(&(text.len() as u32).to_le_bytes())?;
    out.write_all(&text)?;
    for p in payloads {
        out.write_all(p)?;
    }
    out.flush()
}

type Writer = Arc<Mutex<std::io::Stdout>>;

fn send(writer: &Writer, header: Value, payloads: &[&[u8]]) {
    let mut out = writer.lock().unwrap();
    if let Err(e) = write_frame(&mut *out, header, payloads) {
        log(&format!("cannot write a reply: {e}"));
        std::process::exit(2);
    }
}

pub fn log(line: &str) {
    eprintln!("mcbench-draw: {line}");
}

// Fields of a request: `fields` it takes besides id, type and payloads.
fn check_fields(header: &Value, fields: &[&str]) -> Result<(), String> {
    for key in header.as_object().unwrap().keys() {
        if !["id", "type", "payloads"].contains(&key.as_str()) && !fields.contains(&key.as_str()) {
            return Err(format!("A {} request takes no field {key}", header["type"].as_str().unwrap_or("?")));
        }
    }
    Ok(())
}

fn take<T: for<'de> Deserialize<'de>>(header: &Value, name: &str) -> Result<T, String> {
    let v = header.get(name).ok_or_else(|| format!("A {} request needs {name}", header["type"].as_str().unwrap_or("?")))?;
    serde_json::from_value(v.clone()).map_err(|e| format!("{name}: {e}"))
}

struct Ready {
    assets: Assets,
    gpu: Gpu,
    renderer: Renderer,
    font: Arc<Font>,
}

enum State {
    Waiting,
    Ready(Box<Ready>),
    NoAdapter(String),
}

struct Worker {
    // The tick a request began at, and whether it drew (shots), until the
    // trim after its reply has run.
    last: Option<(u64, bool)>,
    state: State,
    store: Store,
    cancelled: Arc<Mutex<HashSet<String>>>,
    writer: Writer,
}

fn id_key(id: &Value) -> String {
    id.to_string()
}

pub fn run() -> Result<(), String> {
    let writer: Writer = Arc::new(Mutex::new(std::io::stdout()));
    let cancelled: Arc<Mutex<HashSet<String>>> = Arc::default();
    let in_flight: Arc<Mutex<HashSet<String>>> = Arc::default();
    let (tx, rx) = mpsc::channel::<Frame>();
    let worker_writer = writer.clone();
    let worker_cancelled = cancelled.clone();
    let worker_flight = in_flight.clone();
    let worker = std::thread::spawn(move || {
        let mut w = Worker { last: None, state: State::Waiting, store: Store::new(store::Limits::new(None, false)), cancelled: worker_cancelled, writer: worker_writer };
        loop {
            // The trim after a reply runs only while no request waits: a
            // reply never waits for memory, nor the next request for the
            // trim of the one before.
            let frame = match rx.try_recv() {
                Ok(frame) => frame,
                Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => {
                    if w.last.is_some() {
                        if let Err(Failure::Device(e)) = w.trim() {
                            log(&format!("the graphics device was lost: {e}; exiting"));
                            std::process::exit(3);
                        }
                        continue;
                    }
                    match rx.recv() {
                        Ok(frame) => frame,
                        Err(_) => break,
                    }
                }
            };
            let id = frame.header["id"].clone();
            let (reply, payloads, lost) = w.answer(&frame);
            worker_flight.lock().unwrap().remove(&id_key(&id));
            w.cancelled.lock().unwrap().remove(&id_key(&id));
            let refs: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
            send(&w.writer, reply, &refs);
            if lost {
                log("the graphics device was lost; exiting");
                std::process::exit(3);
            }
        }
    });
    let mut stdin = std::io::stdin().lock();
    loop {
        let frame = match read_frame(&mut stdin) {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            Err(e) => {
                log(&e);
                std::process::exit(2);
            }
        };
        let id = frame.header.get("id").cloned().unwrap_or(Value::Null);
        if !(id.is_number() || id.is_string()) {
            log("a request without an id (a number or a string)");
            std::process::exit(2);
        }
        if frame.header["type"].as_str() == Some("cancel") {
            let reply = match check_fields(&frame.header, &["job"]).and_then(|_| frame.header.get("job").cloned().ok_or_else(|| "A cancel request needs job".to_string())) {
                Ok(job) => {
                    let found = in_flight.lock().unwrap().contains(&id_key(&job));
                    if found {
                        cancelled.lock().unwrap().insert(id_key(&job));
                    }
                    json!({ "id": id, "ok": true, "found": found })
                }
                Err(e) => json!({ "id": id, "error": e }),
            };
            send(&writer, reply, &[]);
            continue;
        }
        if frame.header["type"].as_str() == Some("shots") {
            in_flight.lock().unwrap().insert(id_key(&id));
        }
        tx.send(frame).expect("the worker is alive");
    }
    drop(tx);
    worker.join().map_err(|_| "The worker panicked".to_string())?;
    Ok(())
}

impl Worker {
    // The reply, its payloads, and whether the device was lost.
    fn answer(&mut self, frame: &Frame) -> (Value, Vec<Vec<u8>>, bool) {
        let id = frame.header["id"].clone();
        let kind = frame.header["type"].as_str().unwrap_or("").to_string();
        let result = match kind.as_str() {
            "hello" => self.hello(frame).map(|r| (r, Vec::new())),
            "montage" => montage(frame),
            "world" | "shots" | "drop" => match &self.state {
                State::Waiting => Err(Failure::Other("The helper has not been given its assets (hello) yet".into())),
                State::NoAdapter(e) => Err(Failure::Other(e.clone())),
                State::Ready(_) => match kind.as_str() {
                    "world" => self.world(frame).map(|r| (r, Vec::new())),
                    "shots" => self.shots(frame),
                    _ => self.drop_world(frame).map(|r| (r, Vec::new())),
                },
            },
            "" => Err(Failure::Other("A request has no type".into())),
            other => Err(Failure::Other(format!("Unknown request type {other}"))),
        };
        match result {
            Ok((mut reply, payloads)) => {
                reply["id"] = id;
                reply["ok"] = json!(true);
                (reply, payloads, false)
            }
            Err(Failure::Other(e)) => {
                let mut reply = json!({ "id": id, "error": e });
                if e.starts_with("No world named") {
                    reply["unknownWorld"] = json!(true);
                }
                (reply, Vec::new(), false)
            }
            Err(Failure::Device(e)) => (json!({ "id": id, "error": format!("The graphics device was lost: {e}"), "deviceLost": true }), Vec::new(), true),
        }
    }

    fn hello(&mut self, frame: &Frame) -> Result<Value, Failure> {
        if !matches!(self.state, State::Waiting) {
            return Err("hello was already given".into());
        }
        check_fields(&frame.header, &["assets", "adapter", "memoryLimit"])?;
        let memory: Option<u64> = match frame.header.get("memoryLimit") {
            Some(_) => Some(take(&frame.header, "memoryLimit")?),
            None => None,
        };
        let paths: Paths = take(&frame.header, "assets")?;
        let software = match frame.header.get("adapter") {
            None => false,
            Some(v) if v == "software" => true,
            Some(v) => return Err(format!("adapter must be \"software\" or absent, not {v}").into()),
        };
        if paths.software_vulkan.is_some() && !cfg!(target_os = "linux") {
            return Err("softwareVulkan is for Linux".into());
        }
        let choice = Choice { software, software_vulkan: paths.software_vulkan.clone() };
        let t0 = crate::now_ms();
        let gpu = match Gpu::open(&choice) {
            Ok(gpu) => gpu,
            Err(e) => {
                log(&e);
                self.state = State::NoAdapter(e.clone());
                return Err(Failure::Other(e));
            }
        };
        let device = crate::now_ms() - t0;
        let report = gpu.report();
        log(&format!("adapter {} ({}, {}, {} {}){}", report.name, report.backend, report.device_type, report.driver, report.driver_info, if report.software { ", a software rasteriser" } else { "" }));
        let t = crate::now_ms();
        let mut assets = Assets::load(&paths)?;
        let assets_ms = crate::now_ms() - t;
        let t = crate::now_ms();
        let renderer = Renderer::new(&gpu, &assets.atlas, crate::gpu::COLOR);
        gpu.wait()?;
        // On the GPU now: the pixels need not be kept.
        assets.atlas.levels = Vec::new();
        let pipelines = crate::now_ms() - t;
        self.store.limits = store::Limits::new(memory, report.software);
        let limits = gpu.device.limits();
        let reply = json!({
            "version": crate::VERSION,
            "adapter": report,
            "limits": {
                "maxTextureSide": limits.max_texture_dimension_2d,
                "maxBufferBytes": limits.max_buffer_size,
                "maxPictureSide": limits.max_texture_dimension_2d,
                "worldsKept": self.store.limits.worlds,
                "scenesKept": self.store.limits.scenes,
                "memoryLimit": self.store.limits.bytes,
            },
            "timings": { "device": crate::round2(device), "assets": crate::round2(assets_ms), "pipelines": crate::round2(pipelines), "total": crate::round2(crate::now_ms() - t0) },
        });
        let font = Arc::new(Font::new()?);
        self.state = State::Ready(Box::new(Ready { assets, gpu, renderer, font }));
        Ok(reply)
    }

    fn ready(&self) -> &Ready {
        match &self.state {
            State::Ready(r) => r,
            _ => unreachable!("checked before"),
        }
    }

    fn world(&mut self, frame: &Frame) -> Result<Value, Failure> {
        check_fields(&frame.header, &["world", "yOffset", "base", "keepBase"])?;
        let name: String = take(&frame.header, "world")?;
        let y_offset: i32 = take(&frame.header, "yOffset")?;
        let base: Option<String> = match frame.header.get("base") {
            Some(_) => Some(take(&frame.header, "base")?),
            None => None,
        };
        let keep_base: bool = match frame.header.get("keepBase") {
            Some(_) => take(&frame.header, "keepBase")?,
            None => false,
        };
        let blueprint = frame.payloads.first().ok_or("A world request needs its blueprint as payload 0")?.clone();
        let entities: Vec<Value> = match frame.payloads.get(1) {
            Some(bytes) => serde_json::from_slice(bytes).map_err(|e| format!("The entities (payload 1) are not a JSON list: {e}"))?,
            None => Vec::new(),
        };
        if frame.payloads.len() > 2 {
            return Err("A world request takes at most two payloads".into());
        }
        if let Some(base) = &base {
            if !self.store.worlds.contains_key(base) {
                return Err(Failure::Other(format!("No world named {base} is kept")));
            }
        }
        let used = self.store.tick();
        let kept = Kept { blueprint, y_offset, entities, used };
        // An edit of a base whose lit scene is kept and that is not kept
        // itself: patched. Otherwise read fresh.
        let edited = match &base {
            Some(base) if !keep_base && self.store.worlds[base].y_offset == y_offset && self.store.scenes.get(&(base.clone(), None)).is_some_and(|s| s.lit.is_some()) => {
                let base_scene = self.store.scenes.remove(&(base.clone(), None)).unwrap();
                let base_kept = self.store.worlds.remove(base).unwrap();
                self.store.drop_world(base);
                Some((base_scene, base_kept))
            }
            _ => None,
        };
        if let (Some(base), false) = (&base, keep_base) {
            self.store.drop_world(base);
        }
        self.store.drop_world(&name);
        let State::Ready(r) = &self.state else { unreachable!("checked before") };
        let store = &mut self.store;
        // Room made for the scene as its buffers are made.
        let mut room = || store.shed(used, false, &mut Measured { r, extra: 0 });
        let (mut scene, times, meshed) = match edited {
            Some((base_scene, base_kept)) => {
                let (scene, times, meshed) = store::patch_scene(&r.assets, &r.gpu, base_scene, &base_kept.blueprint, &kept, &mut room)?;
                (scene, times, Some(meshed))
            }
            None => {
                let (scene, times) = store::make_scene(&r.assets, &r.gpu, &kept, None, &mut room)?;
                (scene, times, None)
            }
        };
        scene.used = used;
        let mut reply = json!({
            "quads": scene.quads(),
            "entityQuads": scene.entity_quads,
            "tiles": scene.tiles.len(),
            "blocks": scene.blocks_count,
            "bounds": { "min": scene.bounds.0, "max": scene.bounds.1 },
            "bytes": scene.bytes(),
            "missingTextures": scene.missing(),
            "timings": { "read": crate::round2(times.read), "mesh": crate::round2(times.mesh), "entities": crate::round2(times.entities), "upload": crate::round2(times.upload), "total": crate::round2(times.total) },
        });
        if let Some(meshed) = meshed {
            reply["patched"] = json!(true);
            reply["tilesMeshed"] = json!(meshed);
        }
        self.store.put_world(&name, kept);
        self.store.put_scene((name, None), scene);
        // What it leaves is trimmed after the reply; the new scene is kept
        // for the shots that follow, even alone over the limit (PROTOCOL.md,
        // What is kept).
        self.last = Some((used, false));
        Ok(reply)
    }

    fn drop_world(&mut self, frame: &Frame) -> Result<Value, Failure> {
        check_fields(&frame.header, &["world"])?;
        let name: String = take(&frame.header, "world")?;
        let dropped = self.store.drop_world(&name);
        self.ready().gpu.wait()?;
        Ok(json!({ "dropped": dropped }))
    }

    // What is kept let go until the process, with `extra` bytes about to
    // be allocated, is within the limit (Store::shed), and the device
    // checked.
    fn shed(&mut self, since: u64, after: bool, extra: u64) -> Result<(), Failure> {
        let State::Ready(r) = &self.state else { unreachable!("checked before") };
        self.store.shed(since, after, &mut Measured { r, extra });
        r.gpu.wait()?;
        Ok(())
    }

    // After a reply, once no request waits (run): what the last request
    // left let go until the process is within the limit.
    fn trim(&mut self) -> Result<(), Failure> {
        if let (Some((since, after)), State::Ready(_)) = (self.last.take(), &self.state) {
            self.shed(since, after, 0)?;
        }
        Ok(())
    }

    fn shots(&mut self, frame: &Frame) -> Result<(Value, Vec<Vec<u8>>), Failure> {
        check_fields(&frame.header, &["world", "shots", "supersample", "format"])?;
        let name: String = take(&frame.header, "world")?;
        let shots: Vec<Shot> = take(&frame.header, "shots")?;
        let supersample: u32 = take(&frame.header, "supersample")?;
        let format: Value = take(&frame.header, "format")?;
        // JPEG (what models get), or PNG: the same picture before the JPEG,
        // for comparing adapters.
        let quality = match format["type"].as_str() {
            Some("jpeg") if format.as_object().is_some_and(|o| o.keys().all(|k| k == "type" || k == "quality")) => Some(format["quality"].as_u64().filter(|q| (1..=100).contains(q)).ok_or("format.quality must be 1 to 100")? as u8),
            Some("png") if format.as_object().is_some_and(|o| o.len() == 1) => None,
            _ => return Err("format must be { type: \"jpeg\", quality } or { type: \"png\" }".into()),
        };
        if !(1..=4).contains(&supersample) {
            return Err("supersample must be 1 to 4".into());
        }
        let id = id_key(&frame.header["id"]);
        if !self.store.worlds.contains_key(&name) {
            return Err(Failure::Other(format!("No world named {name} is kept")));
        }
        let used = self.store.tick();
        self.store.worlds.get_mut(&name).unwrap().used = used;
        // The scenes the shots draw count as drawn by this call; then room
        // made for the picture targets the shots will make.
        for shot in &shots {
            if let Some(scene) = self.store.scenes.get_mut(&(name.clone(), shot.clip)) {
                scene.used = used;
            }
        }
        let sizes: Vec<(u32, u32)> = shots.iter().map(|s| (s.width, s.height)).collect();
        let extra = self.ready().renderer.new_targets_bytes(&self.ready().gpu, &sizes, supersample);
        self.shed(used, false, extra)?;
        let (done_tx, done_rx) = mpsc::channel::<(usize, Result<(Vec<u8>, f64, f64), String>)>();
        let mut timings: Vec<Value> = Vec::new();
        let mut started = Vec::new();
        for (i, shot) in shots.iter().enumerate() {
            if self.cancelled.lock().unwrap().contains(&id) {
                return Err("cancelled".into());
            }
            let t0 = crate::now_ms();
            started.push(t0);
            let key = (name.clone(), shot.clip);
            let mut scene_ms = 0.0;
            if !self.store.scenes.contains_key(&key) {
                let State::Ready(r) = &self.state else { unreachable!("checked before") };
                // The world out of the store while its scene is made (room is
                // made in the store as it grows), then back.
                let kept = self.store.worlds.remove(&name).expect("the world is kept");
                let store = &mut self.store;
                let made = store::make_scene(&r.assets, &r.gpu, &kept, shot.clip, &mut || store.shed(used, false, &mut Measured { r, extra: 0 }));
                self.store.worlds.insert(name.clone(), kept);
                let (scene, times) = made?;
                scene_ms = times.total;
                self.store.put_scene(key.clone(), scene);
            }
            let used = self.store.tick();
            self.store.scenes.get_mut(&key).unwrap().used = used;
            let r = self.ready();
            let scene = &self.store.scenes[&key];
            let (rgba, times) = store::draw_shot(&r.assets, &r.gpu, &r.renderer, scene, shot, supersample)?;
            timings.push(json!({ "width": shot.width, "height": shot.height, "timings": { "scene": crate::round2(scene_ms), "draw": crate::round2(times.draw_ms), "readback": crate::round2(times.readback_ms) } }));
            let grid = shot.grid.clone();
            let (w, h) = (shot.width, shot.height);
            let tx = done_tx.clone();
            let font = self.ready().font.clone();
            rayon::spawn(move || {
                let t = crate::now_ms();
                let rgb = picture::finish(rgba, w, h, grid.as_ref(), &font);
                let grid_ms = crate::now_ms() - t;
                let t = crate::now_ms();
                let encoded = match quality {
                    Some(q) => picture::jpeg(&rgb, w, h, q),
                    None => picture::png(&rgb, w, h),
                };
                let _ = tx.send((i, encoded.map(|bytes| (bytes, grid_ms, crate::now_ms() - t))));
            });
        }
        drop(done_tx);
        let mut pictures: Vec<Option<Vec<u8>>> = vec![None; shots.len()];
        for (i, result) in done_rx {
            let (bytes, grid_ms, encode_ms) = result?;
            timings[i]["bytes"] = json!(bytes.len());
            timings[i]["timings"]["grid"] = json!(crate::round2(grid_ms));
            timings[i]["timings"]["encode"] = json!(crate::round2(encode_ms));
            timings[i]["timings"]["total"] = json!(crate::round2(crate::now_ms() - started[i]));
            pictures[i] = Some(bytes);
        }
        // What is kept let go until the process is within the limit after
        // the reply (PROTOCOL.md, What is kept).
        self.last = Some((used, true));
        Ok((json!({ "shots": timings }), pictures.into_iter().map(|p| p.expect("every shot encoded")).collect()))
    }
}

// The bound measured as the process's own memory (memory::held), after
// the device has freed what was dropped, with `extra` bytes about to be
// allocated.
struct Measured<'a> {
    r: &'a Ready,
    extra: u64,
}

impl store::Bound for Measured<'_> {
    fn over(&mut self, limit: u64) -> bool {
        // A device error is answered by the request's own next wait.
        let _ = self.r.gpu.device.poll(wgpu::PollType::wait_indefinitely());
        if crate::memory::held(self.r.gpu.allocated()) + self.extra <= limit {
            return false;
        }
        // Over: what the allocators keep of freed memory given back first
        // (a malloc_trim costs milliseconds, so only then).
        crate::memory::give_back();
        crate::memory::held(self.r.gpu.allocated()) + self.extra > limit
    }

    fn release_targets(&mut self) -> bool {
        self.r.renderer.release_oldest_targets()
    }
}

fn montage(frame: &Frame) -> Result<(Value, Vec<Vec<u8>>), Failure> {
    check_fields(&frame.header, &["size", "cells", "quality"])?;
    let size: [u32; 2] = take(&frame.header, "size")?;
    let cells: Vec<picture::Cell> = take(&frame.header, "cells")?;
    let quality: u8 = take::<u64>(&frame.header, "quality")?.clamp(0, 255) as u8;
    if !(1..=100).contains(&quality) {
        return Err("quality must be 1 to 100".into());
    }
    let t0 = crate::now_ms();
    let tiles: Vec<&[u8]> = frame.payloads.iter().map(Vec::as_slice).collect();
    let card = picture::montage(&tiles, &cells, size, quality)?;
    Ok((json!({ "timings": { "total": crate::round2(crate::now_ms() - t0) } }), vec![card]))
}
