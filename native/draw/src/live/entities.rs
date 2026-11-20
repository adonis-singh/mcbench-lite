// The live world's entities between ticks: the page's EntityTracker
// (web/entities.mjs:11-89) ported. Each tick's list (the engine's
// `Ticked.entities`) sets where every entity is going; what is drawn glides
// from where it is shown to there over the time the last list took, and
// jumps when the entity moved further than 4 blocks. Their light is read
// from the shown world when a tick's list arrives, for every cell an
// entity's light can be read from on its way (`Lights`).

use crate::entities::{self, BlockQuad, Capture, View};
use crate::scene::Draw;
use mcbench_renderer::world::World;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;

// web/entities.mjs:16: snapDistance 4, defaultInterval 50, minInterval 10,
// maxInterval 250 (blocks and ms).
const SNAP_DISTANCE: f64 = 4.0;
const DEFAULT_INTERVAL: f64 = 50.0;
const MIN_INTERVAL: f64 = 10.0;
const MAX_INTERVAL: f64 = 250.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
}

struct Tween {
    entity: Value,
    from: Pose,
    to: Pose,
    start: f64,
    duration: f64,
}

#[derive(Default)]
pub struct Tracker {
    // By id, in the order ids first came (a JavaScript Map's).
    entities: Vec<(String, Tween)>,
    last_frame: Option<f64>,
}

fn pose(entity: &Value) -> Result<Pose, String> {
    let id = &entity["id"];
    let pos = entity["pos"]
        .as_array()
        .filter(|p| p.len() == 3)
        .and_then(|p| p.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>())
        .ok_or_else(|| format!("Entity {id} has no position"))?;
    let (yaw, pitch) = (entity["yaw"].as_f64(), entity["pitch"].as_f64());
    match (yaw, pitch) {
        (Some(yaw), Some(pitch)) => Ok(Pose {
            pos: [pos[0], pos[1], pos[2]],
            yaw,
            pitch,
        }),
        _ => Err(format!("Entity {id} has no angles")),
    }
}

impl Tracker {
    // A tick's whole list at `now` (ms): web/entities.mjs set (27-59).
    pub fn set(&mut self, list: Vec<Value>, now: f64) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        let mut poses = Vec::with_capacity(list.len());
        for entity in &list {
            let id = entity["id"].to_string();
            if !seen.insert(id.clone()) {
                return Err(format!("Entity {id} is in the frame twice"));
            }
            poses.push(pose(entity)?);
        }
        let duration = match self.last_frame {
            None => DEFAULT_INTERVAL,
            Some(last) => (now - last).clamp(MIN_INTERVAL, MAX_INTERVAL),
        };
        self.last_frame = Some(now);
        self.entities.retain(|(id, _)| seen.contains(id));
        for (entity, target) in list.into_iter().zip(poses) {
            let id = entity["id"].to_string();
            match self.entities.iter_mut().find(|(i, _)| *i == id) {
                None => self.entities.push((
                    id,
                    Tween {
                        entity,
                        from: target,
                        to: target,
                        start: now,
                        duration,
                    },
                )),
                Some((_, known)) => {
                    let shown = sample_one(known, now);
                    let jumped = (0..3)
                        .map(|i| (target.pos[i] - known.to.pos[i]).powi(2))
                        .sum::<f64>()
                        .sqrt()
                        > SNAP_DISTANCE;
                    known.entity = entity;
                    known.from = if jumped { target } else { shown };
                    known.to = target;
                    known.start = now;
                    known.duration = duration;
                }
            }
        }
        Ok(())
    }

    // Every entity as drawn at `now`, its JSON with the pose replaced.
    pub fn sample(&self, now: f64) -> Vec<Value> {
        self.entities
            .iter()
            .map(|(_, t)| with_pose(&t.entity, sample_one(t, now)))
            .collect()
    }

    // web/entities.mjs moving (80-89): whether any entity still glides at
    // `now`, as 32-bit floats draw it.
    pub fn moving(&self, now: f64) -> bool {
        let drawn = |p: Pose| [p.pos[0], p.pos[1], p.pos[2], p.yaw, p.pitch].map(|v| v as f32);
        self.entities.iter().any(|(_, t)| {
            now - t.start < t.duration
                && drawn(sample_one(t, t.start)) != drawn(sample_one(t, t.start + t.duration))
        })
    }

    // Each entity's poses at the start and end of its glide.
    fn ends(&self) -> impl Iterator<Item = (&Value, Pose, Pose)> {
        self.entities.iter().map(|(_, t)| (&t.entity, t.from, t.to))
    }
}

// web/entities.mjs sampleOne (61-69): positions linear, the yaw the short
// way round, the pitch linear.
fn sample_one(t: &Tween, now: f64) -> Pose {
    let k = ((now - t.start) / t.duration).clamp(0.0, 1.0);
    let turn = |from: f64, to: f64| from + (((to - from + 540.0) % 360.0) - 180.0) * k;
    Pose {
        pos: [0, 1, 2].map(|i| t.from.pos[i] + (t.to.pos[i] - t.from.pos[i]) * k),
        yaw: turn(t.from.yaw, t.to.yaw),
        pitch: t.from.pitch + (t.to.pitch - t.from.pitch) * k,
    }
}

fn with_pose(entity: &Value, p: Pose) -> Value {
    let mut e = entity.clone();
    e["pos"] = serde_json::json!(p.pos);
    e["yaw"] = serde_json::json!(p.yaw);
    e["pitch"] = serde_json::json!(p.pitch);
    e
}

// The shown world's light where the tracked entities read it, taken when a
// tick's list arrives (the world is then on the render thread): every cell
// of the box each entity's light cells span between its glide's two ends.
// What see_build's entities read (store.rs WorldView): the brighter of sky
// and block light as both, and the cell's own levels.
// By cell: what see_build's View::light answers and the cell's levels.
pub type Cells = HashMap<[i32; 3], ([i32; 2], [i32; 2])>;

pub struct Lights<'a> {
    pub cells: Cells,
    pub pack: &'a mcbench_renderer::pack::Pack,
    pub names: &'a [String],
    pub lone: RefCell<HashMap<String, Vec<BlockQuad>>>,
}

pub fn lights(
    capture: &Capture,
    tracker: &Tracker,
    world: &World,
    draw: &Draw,
) -> Result<Cells, String> {
    let mut cells = HashMap::new();
    for (entity, from, to) in tracker.ends() {
        let a = entities::light_cells(capture, &with_pose(entity, from))?;
        let b = entities::light_cells(capture, &with_pose(entity, to))?;
        for (p, q) in a.iter().zip(&b) {
            for x in p[0].min(q[0])..=p[0].max(q[0]) {
                for y in p[1].min(q[1])..=p[1].max(q[1]) {
                    for z in p[2].min(q[2])..=p[2].max(q[2]) {
                        let level = draw.cell_light(world, [x, y, z]);
                        let [sky, block] = draw.cell_levels(world, [x, y, z]);
                        cells.insert([x, y, z], ([level, level], [block as i32, sky as i32]));
                    }
                }
            }
        }
    }
    Ok(cells)
}

impl View for Lights<'_> {
    fn light(&self, cell: [i32; 3]) -> [i32; 2] {
        self.cells
            .get(&cell)
            .unwrap_or_else(|| {
                panic!("the light of cell {cell:?} was taken with the tick's entities")
            })
            .0
    }
    fn levels(&self, cell: [i32; 3]) -> [i32; 2] {
        self.cells
            .get(&cell)
            .unwrap_or_else(|| {
                panic!("the light of cell {cell:?} was taken with the tick's entities")
            })
            .1
    }
    fn block_quads(&self, state: &str) -> Result<Vec<BlockQuad>, String> {
        if let Some(q) = self.lone.borrow().get(state) {
            return Ok(q.clone());
        }
        let quads = entities::lone_quads(self.pack, self.names, state)?;
        self.lone
            .borrow_mut()
            .insert(state.to_string(), quads.clone());
        Ok(quads)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_entity_glides_between_two_lists_and_jumps_past_four_blocks() {
        let mut t = Tracker::default();
        t.set(
            vec![json!({"id": 1, "pos": [0.0, 0.0, 0.0], "yaw": 350.0, "pitch": 0.0})],
            0.0,
        )
        .unwrap();
        t.set(
            vec![json!({"id": 1, "pos": [2.0, 0.0, 0.0], "yaw": 10.0, "pitch": 0.0})],
            100.0,
        )
        .unwrap();
        let mid = t.sample(150.0);
        assert_eq!(
            mid[0]["pos"][0].as_f64(),
            Some(1.0),
            "halfway over the 100 ms the last list took"
        );
        assert_eq!(mid[0]["yaw"].as_f64(), Some(360.0), "the short way round");
        assert!(t.moving(150.0) && !t.moving(200.0));
        t.set(
            vec![json!({"id": 1, "pos": [9.0, 0.0, 0.0], "yaw": 10.0, "pitch": 0.0})],
            300.0,
        )
        .unwrap();
        assert_eq!(t.sample(300.0)[0]["pos"][0].as_f64(), Some(9.0), "a jump");
    }
}
