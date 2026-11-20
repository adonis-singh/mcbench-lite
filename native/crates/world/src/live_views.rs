//! What each viewer is shown, as versions of a build
//! (`server/live-views.js`). A view is a conversation or a bench round (the
//! page names it by `key`); its version changes whenever its build changes
//! enough to place, and the page asks for the commands that take the world
//! it holds, at version `since`, to the latest: the live import's commands
//! ([`crate::live_placement`]), or a fresh placement when the page holds
//! nothing or a version no longer kept.

use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::dimension::validate_world_height;
use crate::facts::LastPass;
use crate::live_placement::import_commands;
use crate::{BlockBox, World};

/// Versions kept per view (`VERSIONS_KEPT`).
pub const VERSIONS_KEPT: usize = 16;
/// A bench round's view is composed at most this often (`LOOK_MS`).
pub const LOOK_MS: f64 = 250.0;
/// Views kept at all, the least recently looked at going first
/// (`VIEWS_KEPT`).
pub const VIEWS_KEPT: usize = 64;

/// The area BlueMap draws around a build (`server/bluemap.js` `renderMask`),
/// which a view's descriptor carries: `{ minX, maxX, minZ, maxZ }`.
pub fn render_mask(bounds: Option<BlockBox>) -> Value {
    let b = bounds.unwrap_or(BlockBox {
        min: [0; 3],
        max: [0; 3],
    });
    let span = (b.max[0] - b.min[0] + 1).max(b.max[2] - b.min[2] + 1) as i64;
    let mut size: i64 = 128;
    while size < span + 64 {
        size *= 2;
    }
    let center_x = (b.min[0] as f64 + b.max[0] as f64) / 2.0;
    let center_z = (b.min[2] as f64 + b.max[2] as f64) / 2.0;
    let min_x = ((center_x - size as f64 / 2.0) / 16.0).floor() as i64 * 16;
    let min_z = ((center_z - size as f64 / 2.0) / 16.0).floor() as i64 * 16;
    json!({ "minX": min_x, "maxX": min_x + size - 1, "minZ": min_z, "maxZ": min_z + size - 1 })
}

fn bounds_json(bounds: Option<BlockBox>) -> Value {
    bounds.map_or(Value::Null, |b| json!({ "min": b.min, "max": b.max }))
}

struct Version {
    version: u64,
    world: World,
    layout: Value,
    fresh: Option<String>,
}

#[derive(Default)]
struct View {
    versions: Vec<Version>,
    source: Option<u64>,
    revision: Option<i64>,
    checked: f64,
    layout: Value,
    descriptor: Option<Value>,
}

/// What a view resolves to: its world and the bench's layout (`null` for a
/// chat), or the error (a view that does not exist).
pub type Resolved = Result<(Arc<World>, Value), String>;

/// The commands of [`LiveViews::imports`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imports {
    /// The latest version.
    pub version: u64,
    /// Whether the commands place the build afresh.
    pub fresh: bool,
    /// The import's lines.
    pub commands: String,
    /// The latest build's blocks.
    pub block_count: usize,
}

/// The views (`LiveViews`).
pub struct LiveViews<R: FnMut(&str) -> Resolved> {
    resolve: R,
    y_offset: i32,
    now: Box<dyn FnMut() -> f64 + Send>,
    facts: Arc<dyn LastPass + Send + Sync>,
    views: IndexMap<String, View>,
    counter: u64,
    frames: HashMap<String, u64>,
}

fn js_json(value: &Value) -> String {
    serde_json::to_string(value).expect("JSON")
}

impl<R: FnMut(&str) -> Resolved> LiveViews<R> {
    /// Views over `resolve`, at `y_offset`, with the clock `now` (ms) and
    /// the import's `facts`.
    pub fn new(
        resolve: R,
        y_offset: i32,
        now: Box<dyn FnMut() -> f64 + Send>,
        facts: Arc<dyn LastPass + Send + Sync>,
    ) -> LiveViews<R> {
        LiveViews {
            resolve,
            y_offset,
            now,
            facts,
            views: IndexMap::new(),
            counter: 0,
            frames: HashMap::new(),
        }
    }

    /// How many views are kept.
    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// Whether none is kept.
    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    /// The view's latest version, made if its build changed since the last
    /// look (`look`).
    fn look(&mut self, key: &str) -> Result<(), String> {
        if let Some(view) = self.views.get(key)
            && key.starts_with("bench:")
            && (self.now)() - view.checked < LOOK_MS
        {
            return Ok(());
        }
        let (world, layout) = (self.resolve)(key)?;
        let mut view = self.views.shift_remove(key).unwrap_or_default();
        view.checked = (self.now)();
        let unchanged = view.source == Some(world.uid())
            && view.revision == Some(world.revision())
            && js_json(&view.layout) == js_json(&layout);
        if unchanged && view.descriptor.is_some() {
            self.keep(key, view);
            return Ok(());
        }
        let result = self.update(key, &mut view, &world, layout);
        self.keep(key, view);
        result
    }

    fn keep(&mut self, key: &str, view: View) {
        self.views.insert(key.to_string(), view);
        while self.views.len() > VIEWS_KEPT {
            self.views.shift_remove_index(0);
        }
    }

    fn update(
        &mut self,
        key: &str,
        view: &mut View,
        world: &World,
        layout: Value,
    ) -> Result<(), String> {
        validate_world_height(world, self.y_offset)?;
        let reference = world.reference();
        view.source = Some(world.uid());
        view.revision = Some(world.revision());
        view.layout = layout;
        if let Some(last) = view.versions.last()
            && js_json(&last.layout) == js_json(&view.layout)
            && import_commands(
                &reference,
                Some(&last.world),
                self.y_offset,
                None,
                &*self.facts,
            )?
            .0
            .is_empty()
        {
            return Ok(());
        }
        let bounds = reference.bounds();
        self.counter += 1;
        let block_count = reference.size();
        view.versions.push(Version {
            version: self.counter,
            world: reference,
            layout: view.layout.clone(),
            fresh: None,
        });
        while view.versions.len() > VERSIONS_KEPT {
            view.versions.remove(0);
        }
        view.descriptor = Some(json!({
            "key": key,
            "version": self.counter,
            "mask": render_mask(bounds),
            "bounds": bounds_json(bounds),
            "blockCount": block_count,
            "layout": view.layout,
        }));
        Ok(())
    }

    fn frame_key(key: &str) -> String {
        if key.starts_with("bench:") {
            key.split(':').take(2).collect::<Vec<_>>().join(":")
        } else {
            key.to_string()
        }
    }

    /// The view as the page lists it (`describe`): its descriptor, `frame`
    /// and `syncError` (a build that cannot be placed keeps the version
    /// before it and says why), as JSON in today's key order.
    pub fn describe(&mut self, key: &str) -> Result<Value, String> {
        let mut sync_error = Value::Null;
        if let Err(error) = self.look(key) {
            if self
                .views
                .get(key)
                .and_then(|v| v.descriptor.as_ref())
                .is_none()
            {
                return Err(error);
            }
            sync_error = Value::String(format!("Minecraft kept the previous build: {error}"));
        }
        let mut out = self.views[key].descriptor.clone().expect("described");
        let frame = self.frames.get(&Self::frame_key(key)).copied().unwrap_or(0);
        let object = out.as_object_mut().expect("an object");
        object.insert("frame".into(), json!(frame));
        object.insert("syncError".into(), sync_error);
        Ok(out)
    }

    /// The commands that take the world at version `since` (or nothing) to
    /// the latest (`imports`).
    pub fn imports(&mut self, key: &str, since: Option<u64>) -> Result<Imports, String> {
        self.describe(key)?;
        let y_offset = self.y_offset;
        let facts = self.facts.clone();
        let view = self.views.get_mut(key).expect("described");
        let latest_at = view.versions.len() - 1;
        let held = since.and_then(|since| view.versions.iter().position(|v| v.version == since));
        let block_count = view.versions[latest_at].world.size();
        let version = view.versions[latest_at].version;
        if held == Some(latest_at) {
            return Ok(Imports {
                version,
                fresh: false,
                commands: String::new(),
                block_count,
            });
        }
        let Some(held) = held else {
            let latest = &mut view.versions[latest_at];
            if latest.fresh.is_none() {
                latest.fresh = Some(
                    latest
                        .world
                        .place_commands(None, y_offset, None, &*facts)?
                        .0,
                );
            }
            return Ok(Imports {
                version,
                fresh: true,
                commands: latest.fresh.clone().expect("made above"),
                block_count,
            });
        };
        let commands = view.versions[latest_at]
            .world
            .place_commands(Some(&view.versions[held].world), y_offset, None, &*facts)?
            .0;
        Ok(Imports {
            version,
            fresh: false,
            commands,
            block_count,
        })
    }

    /// Asks the viewers of a view to move the camera to it once its current
    /// version is on screen; benches count for all their rounds (`frame`).
    pub fn frame(&mut self, key: &str) {
        *self.frames.entry(Self::frame_key(key)).or_insert(0) += 1;
    }
}
