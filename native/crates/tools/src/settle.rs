//! The build as Minecraft makes it, which `build` reports and reads back
//! (`mc.settled`) and `see_build` will draw: `server/shot-world.js`
//! `settle`, `runSettle`, `checkTicks`, `currentTicks` and `idChanges` over
//! `mcbench-engine`'s settle pool ([`SettlePool`]) and what it keeps of each
//! conversation ([`Settles`]). The engine places the world's blocks as the
//! live import does, runs the ticks asked in one run to the largest, and
//! writes the world at each; this module reads each blueprint back as a
//! [`World`] with the conversation's latest palette, compares its block ids
//! with the world placed, and reads the entities and the blocks that cannot
//! survive where they are.
//!
//! `adoptUnchanged` (a settled world taking the previous settle's chunk
//! objects, so the page keeps their meshes) is not ported: nothing a model
//! or the interface reads depends on which object a chunk is. The settle's
//! timings (`[build] ... settle ...`, a server log line) are not kept.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use mcbench_engine::{SettlePool, Settles, SettlesKey};
use mcbench_world::{BlockPos, World, coords_of};
use serde_json::{Value, json};

use mcbench_world::facts::MinecraftFacts;

/// `CHANGE_EXAMPLES`: the examples of each kind of change kept.
pub const CHANGE_EXAMPLES: usize = 8;

/// `DEFAULT_SETTLE_TICKS`.
pub const DEFAULT_SETTLE_TICKS: i64 = mcbench_world::dimension::DEFAULT_SETTLE_TICKS as i64;

/// Where the settled world's block ids differ from the world placed
/// (`idChanges`): counts of blocks removed, added and replaced, with the
/// first [`CHANGE_EXAMPLES`] of each as today's JSON (`{at, was}`, `{at,
/// now}`, `{at, was, now}`).
#[derive(Clone, Debug, PartialEq)]
pub struct Changes {
    /// `removed`.
    pub removed: u64,
    /// `added`.
    pub added: u64,
    /// `replaced`.
    pub replaced: u64,
    /// `examples`: `{removed: [...], added: [...], replaced: [...]}`.
    pub examples: Value,
}

/// What `see_build`'s renderer draws of a settle (`sceneOf`): the engine's
/// blueprint bytes at Minecraft's heights, a number naming the settle among
/// those this process made, and its edition: which authored world, at which
/// revision, it was settled from (the settles of one edition at different
/// ticks are the same build).
#[derive(Debug)]
pub struct Scene {
    /// `serial`.
    pub serial: u64,
    /// `edition`: the authored world's [`World::uid`] and revision.
    pub edition: (u64, i64),
    /// The engine's blueprint (empty for a world of no blocks).
    pub blueprint: Vec<u8>,
    /// The height offset the blueprint is at (`minecraftLive.yOffset`).
    pub y_offset: i32,
}

/// The settles this process made (`serials`).
static SERIALS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// One settle of a world after some ticks (`runSettle`'s result).
#[derive(Debug)]
pub struct SettledWorld {
    /// The engine's world after the ticks, at the world's heights.
    pub world: World,
    /// Its entities then (the engine's list, heights the world's).
    pub entities: Vec<Value>,
    /// Which block ids changed from the world placed.
    pub changes: Changes,
    /// The blocks that cannot survive where they are (the engine's
    /// `SURVIVE`), at the world's heights.
    pub unsupported: Vec<BlockPos>,
    /// The ticks run.
    pub ticks: i64,
    /// What the renderer draws of it.
    pub scene: Scene,
}

/// A settle as [`Settler::settle`] answers it.
#[derive(Clone, Debug)]
pub struct Settle {
    /// The settle.
    pub value: Arc<SettledWorld>,
    /// It was made before this call.
    pub cached: bool,
}

/// `checkTicks(value, label)`: a whole number of ticks within what
/// `test_build` may simulate (`max`, `MCBENCH_VERIFY_MAX_SIMULATION_TICKS`),
/// with today's sentences.
pub fn check_ticks(value: &Value, label: &str, max: u64) -> Result<i64, String> {
    let ticks = crate::js::integer(value)
        .filter(|ticks| *ticks >= 0.0)
        .ok_or_else(|| format!("{label} must be a whole number of ticks, 0 or more"))?;
    if ticks > max as f64 {
        return Err(format!(
            "{label} is {} ticks; the budget of simulated ticks, the same as test_build's, is {max} (MCBENCH_VERIFY_MAX_SIMULATION_TICKS)",
            mcbench_json::number_to_string(ticks)
        ));
    }
    Ok(ticks as i64)
}

/// `currentTicks(world)`: the ticks the build that made `world` settled it
/// with, or the default.
pub fn current_ticks(world: &World) -> i64 {
    world.settle_ticks.unwrap_or(DEFAULT_SETTLE_TICKS)
}

/// The settles of the app: the engine's pool and what is kept of each
/// conversation.
pub struct Settler {
    pool: SettlePool,
    kept: Settles<Arc<SettledWorld>>,
    facts: Arc<MinecraftFacts>,
    y_offset: i32,
    max_ticks: u64,
}

impl Settler {
    /// Settles on `pool`, the import ordered by `facts`, at Minecraft height
    /// `y_offset` (`minecraftLive.yOffset`), within `max_ticks`
    /// (`BUDGETS.simulationTicks`).
    pub fn new(
        pool: SettlePool,
        facts: Arc<MinecraftFacts>,
        y_offset: i32,
        max_ticks: u64,
    ) -> Settler {
        Settler {
            pool,
            kept: Settles::new(),
            facts,
            y_offset,
            max_ticks,
        }
    }

    /// The height offset the settles are at (`minecraftLive.yOffset`).
    pub fn y_offset(&self) -> i32 {
        self.y_offset
    }

    /// The settle pool.
    pub fn pool(&self) -> &SettlePool {
        &self.pool
    }

    /// `settle(conversationId, world, ticks, { primary })`: the settles of
    /// `world`, the conversation's current world, for each of `ticks`; those
    /// made before are kept, the rest are made now in one run. `primary`,
    /// when given, becomes the world's settle ticks (set before the run, so
    /// a settle that fails still sets them, as today).
    pub async fn settle(
        &self,
        conversation: &str,
        world: &mut World,
        ticks: &[i64],
        primary: Option<i64>,
    ) -> Result<BTreeMap<i64, Settle>, String> {
        let mut wanted = ticks.to_vec();
        wanted.sort();
        wanted.dedup();
        for &tick in &wanted {
            check_ticks(&json!(tick), "ticks", self.max_ticks)?;
        }
        if let Some(primary) = primary {
            world.settle_ticks = Some(check_ticks(&json!(primary), "ticks", self.max_ticks)?);
        }
        let key = SettlesKey {
            world: world.uid(),
            revision: world.revision() as u64,
        };
        let placed = world.clone();
        let pool = self.pool.clone();
        let facts = self.facts.clone();
        let y_offset = self.y_offset;
        let id = conversation.to_string();
        let wanted32: Vec<i32> = wanted.iter().map(|&t| t as i32).collect();
        let answer = self
            .kept
            .settle(conversation, key, &wanted32, move |latest, missing| {
                run_settle(pool, facts, y_offset, id, placed, latest, missing)
            })
            .await
            .map_err(|error| error.to_string())?;
        Ok(answer
            .into_iter()
            .map(|settle| {
                (
                    i64::from(settle.ticks),
                    Settle {
                        value: settle.value,
                        cached: settle.cached,
                    },
                )
            })
            .collect())
    }
}

/// `runSettle`: the engine's settle of `world` for each of `ticks` on the
/// pool, read back as worlds with the conversation's latest palette, which
/// each result then becomes.
async fn run_settle(
    pool: SettlePool,
    facts: Arc<MinecraftFacts>,
    y_offset: i32,
    conversation: String,
    world: World,
    latest: Arc<Mutex<Option<Arc<SettledWorld>>>>,
    ticks: Vec<i32>,
) -> Result<Vec<(i32, Arc<SettledWorld>)>, mcbench_engine::EngineError> {
    let failed = mcbench_engine::EngineError::Stopped;
    let commands = if world.size() > 0 {
        world
            .place_commands(None, y_offset, None, &*facts)
            .map_err(failed)?
    } else {
        mcbench_world::PlaceCommands(String::new())
    };
    let settled = pool.settle(&conversation, commands, ticks).await?;
    let mut out = Vec::new();
    for one in settled {
        let previous = latest.lock().expect("latest settle").clone();
        let palette = previous.map(|previous| previous.world.palette().clone());
        let read = if one.blueprint.0.is_empty() {
            World::with_palette(palette.unwrap_or_default())
        } else {
            World::from_blueprint(&one.blueprint, y_offset, palette).map_err(failed)?
        };
        let entities: Vec<Value> = if one.blueprint.0.is_empty() {
            Vec::new()
        } else {
            let list: Value = mcbench_json::parse(
                std::str::from_utf8(&one.entities)
                    .map_err(|_| failed("The engine's entities are not UTF-8".into()))?,
            )
            .map_err(failed)?;
            let Value::Array(list) = list else {
                return Err(failed("The engine's entities are not a list".into()));
            };
            list.into_iter()
                .map(|mut entity| {
                    if let Some(pos) = entity.get_mut("pos").and_then(Value::as_array_mut)
                        && let Some(y) = pos.get(1).and_then(Value::as_f64)
                    {
                        pos[1] = json!(y - f64::from(y_offset));
                    }
                    entity
                })
                .collect()
        };
        let unsupported = one
            .unsupported
            .split('\n')
            .filter(|line| !line.is_empty())
            .map(|line| {
                let numbers: Vec<i32> = line
                    .split(' ')
                    .map(|n| {
                        n.parse()
                            .map_err(|_| failed(format!("SURVIVE wrote {line}")))
                    })
                    .collect::<Result<_, _>>()?;
                match numbers[..] {
                    [x, y, z] => Ok([x, y - y_offset, z]),
                    _ => Err(failed(format!("SURVIVE wrote {line}"))),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let changes = id_changes(&world, &read);
        let result = Arc::new(SettledWorld {
            world: read,
            entities,
            changes,
            unsupported,
            ticks: i64::from(one.ticks),
            scene: Scene {
                serial: SERIALS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1,
                edition: (world.uid(), world.revision()),
                blueprint: one.blueprint.0,
                y_offset,
            },
        });
        *latest.lock().expect("latest settle") = Some(result.clone());
        out.push((one.ticks, result));
    }
    Ok(out)
}

/// `idChanges(authored, settled)`: where the settled world's block ids
/// differ from the authored world's, cell by cell, chunks in the authored
/// world's order then the settled world's new ones.
pub fn id_changes(authored: &World, settled: &World) -> Changes {
    let mut keys = indexmap::IndexSet::<u32>::new();
    keys.extend(authored.chunks().map(|(key, _)| key));
    keys.extend(settled.chunks().map(|(key, _)| key));
    // Each palette entry's block id as a number (air is 0).
    let mut numbers: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut ids_of = |world: &World| -> Vec<u32> {
        let palette = world.palette();
        let mut ids = vec![0u32; palette.len().max(1)];
        for (index, slot) in ids.iter_mut().enumerate().skip(1) {
            let id = &palette.entry(index).id;
            let next = numbers.len() as u32 + 1;
            *slot = *numbers.entry(id.clone()).or_insert(next);
        }
        ids
    };
    let authored_ids = ids_of(authored);
    let settled_ids = ids_of(settled);
    let empty = [0u16; mcbench_world::CHUNK_VOLUME];
    let (mut removed, mut added, mut replaced) = (0u64, 0u64, 0u64);
    let mut examples: [Vec<Value>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let id = |world: &World, value: u16| world.palette().entry(value as usize).id.clone();
    for key in keys {
        let before = authored.chunk(key).map(|chunk| chunk.view());
        let after = settled.chunk(key).map(|chunk| chunk.view());
        let before: &[u16; mcbench_world::CHUNK_VOLUME] = before.as_deref().unwrap_or(&empty);
        let after: &[u16; mcbench_world::CHUNK_VOLUME] = after.as_deref().unwrap_or(&empty);
        let [cx, cy, cz] = coords_of(key);
        for index in 0..mcbench_world::CHUNK_VOLUME {
            let was = authored_ids[before[index] as usize];
            let now = settled_ids[after[index] as usize];
            if was == now {
                continue;
            }
            let kind = if now == 0 {
                removed += 1;
                0
            } else if was == 0 {
                added += 1;
                1
            } else {
                replaced += 1;
                2
            };
            if examples[kind].len() < CHANGE_EXAMPLES {
                let at = json!([
                    cx * 16 + (index & 15) as i32,
                    cy * 16 + (index >> 8) as i32,
                    cz * 16 + ((index >> 4) & 15) as i32
                ]);
                examples[kind].push(match kind {
                    0 => json!({"at": at, "was": id(authored, before[index])}),
                    1 => json!({"at": at, "now": id(settled, after[index])}),
                    _ => json!({
                        "at": at,
                        "was": id(authored, before[index]),
                        "now": id(settled, after[index])
                    }),
                });
            }
        }
    }
    let [removed_examples, added_examples, replaced_examples] = examples;
    Changes {
        removed,
        added,
        replaced,
        examples: json!({
            "removed": removed_examples,
            "added": added_examples,
            "replaced": replaced_examples
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_checked_as_check_ticks_checks_them() {
        assert_eq!(check_ticks(&json!(210), "settle_ticks", 72_000), Ok(210));
        assert_eq!(check_ticks(&json!(210.0), "settle_ticks", 72_000), Ok(210));
        for bad in [json!(-1), json!(1.5), json!("210"), json!(null), json!([1])] {
            assert_eq!(
                check_ticks(&bad, "settle_ticks", 72_000),
                Err("settle_ticks must be a whole number of ticks, 0 or more".into())
            );
        }
        assert_eq!(
            check_ticks(&json!(1e21), "tick", 72_000),
            Err("tick is 1e+21 ticks; the budget of simulated ticks, the same as test_build's, is 72000 (MCBENCH_VERIFY_MAX_SIMULATION_TICKS)".into())
        );
    }

    #[test]
    fn changed_ids_are_counted_cell_by_cell_with_the_first_examples() {
        let stone = mcbench_world::BlockState::new("minecraft:stone", &[]);
        let mut authored = World::new();
        authored.set([0, 0, 0], stone.clone()).unwrap();
        authored.set([1, 0, 0], stone.clone()).unwrap();
        authored
            .set(
                [2, 0, 0],
                mcbench_world::BlockState::new("minecraft:oak_stairs", &[("facing", "north")]),
            )
            .unwrap();
        let mut settled = World::new();
        settled
            .set(
                [1, 0, 0],
                mcbench_world::BlockState::new("minecraft:dirt", &[]),
            )
            .unwrap();
        settled
            .set(
                [2, 0, 0],
                mcbench_world::BlockState::new("minecraft:oak_stairs", &[("facing", "east")]),
            )
            .unwrap();
        settled.set([0, 1, 0], stone).unwrap();
        let changes = id_changes(&authored, &settled);
        assert_eq!(
            (changes.removed, changes.added, changes.replaced),
            (1, 1, 1)
        );
        assert_eq!(
            mcbench_json::stringify(&changes.examples),
            r#"{"removed":[{"at":[0,0,0],"was":"minecraft:stone"}],"added":[{"at":[0,1,0],"now":"minecraft:stone"}],"replaced":[{"at":[1,0,0],"was":"minecraft:stone","now":"minecraft:dirt"}]}"#
        );
    }
}
