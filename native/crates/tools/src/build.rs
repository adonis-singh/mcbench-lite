//! `build`: `agent.js` `executeBuild` with `minecraftReport`,
//! `incompleteBed` and `unsupportedBlocks`. The program runs in the build
//! child (`mcbench_build::BuildRunner`) on a clone of the chat's world;
//! Minecraft settles the world it starts from (kept from the last build's
//! settle) for `mc.settled`, and a program that changes the world is
//! settled before it is applied, so a build is applied only with
//! Minecraft's verdict on it. What it writes counts against the chat's
//! storage budget before any of it is applied.

use std::sync::Arc;

use async_trait::async_trait;
use mcbench_build::{BuildHost, BuildRequest};
use mcbench_world::{BlockPos, World, key_of, world::KeySet};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::settle::{self, SettledWorld};
use crate::{Chat, Progress, Services, Tool, ToolError, ToolOutcome, Workspace, js};
use mcbench_world::facts::MinecraftFacts;

/// `MAX_ERROR_CHARS`: a build error's message and stack, as the model sees
/// it, in UTF-16 code units.
pub const MAX_ERROR_CHARS: usize = 8000;
/// `SHOWN_BLOCKS`: the unsupported blocks a refusal names.
pub const SHOWN_BLOCKS: usize = 8;

const NOT_RUN: &str = "program not run — no blocks or files were changed";
const ABORTED: &str = "program aborted — no blocks or files were changed";

/// The `build` tool.
pub struct Build(pub Arc<Services>);

/// What the child asks of the parent during a build: `mc.readFile` from
/// `/work` through the symlink-safe reader, `mc.settled` from the settle of
/// the world the program started from.
struct Host<'a> {
    workspace: &'a Workspace,
    settled: World,
}

#[async_trait]
impl BuildHost for Host<'_> {
    async fn read_file(&mut self, path: &str) -> Result<String, String> {
        mcbench_sandbox::work::read_file_for_build(self.workspace, path)
    }

    async fn settled_world(&mut self) -> Result<World, String> {
        Ok(self.settled.clone())
    }
}

fn failure(error: String, note: &str) -> Value {
    json!({"ok": false, "error": error, "note": note})
}

/// `minecraftReport(settled)`: what Minecraft made of a build, as the model
/// reads it.
pub fn minecraft_report(settled: &SettledWorld) -> Value {
    json!({
        "ticks": settled.ticks,
        "blocks": settled.world.size(),
        "removed": settled.changes.removed,
        "added": settled.changes.added,
        "replaced": settled.changes.replaced,
        "examples": settled.changes.examples,
    })
}

/// `FACING_STEP`.
fn facing_step(facing: &str) -> Result<[i32; 3], String> {
    Ok(match facing {
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "east" => [1, 0, 0],
        "west" => [-1, 0, 0],
        other => {
            return Err(format!(
                "Cannot read properties of undefined (reading '0'): facing {other}"
            ));
        }
    })
}

/// `{ ...facts.defaults(id), ...states }[name]`.
fn merged(
    facts: &MinecraftFacts,
    id: &str,
    states: &[(String, String)],
    name: &str,
) -> Result<Option<String>, String> {
    if let Some((_, value)) = states.iter().find(|(key, _)| key == name) {
        return Ok(Some(value.clone()));
    }
    Ok(facts
        .defaults(id)?
        .into_iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value))
}

/// `incompleteBed(world, near)`: a bed half whose partner is missing or
/// mismatched, in the chunks `near` names (a bed is a block of Minecraft's
/// BedBlock; the head sits one block from the foot toward its facing).
pub fn incomplete_bed(
    world: &World,
    near: &KeySet,
    facts: &MinecraftFacts,
) -> Result<Option<String>, String> {
    let is_bed = |id: &str| facts.is(id, "BedBlock");
    let mut any = false;
    for entry in world.palette().entries() {
        if is_bed(&entry.id)? {
            any = true;
            break;
        }
    }
    if !any {
        return Ok(None);
    }
    for &key in near {
        let mut found: Option<String> = None;
        let mut failed: Option<String> = None;
        world.for_each_in_chunk(key, |[x, y, z], block| {
            if found.is_some() || failed.is_some() {
                return;
            }
            let check = || -> Result<Option<String>, String> {
                if !is_bed(&block.id)? {
                    return Ok(None);
                }
                let facing = merged(facts, &block.id, &block.properties, "facing")?
                    .unwrap_or_else(|| "undefined".into());
                let part = merged(facts, &block.id, &block.properties, "part")?
                    .unwrap_or_else(|| "undefined".into());
                let partner = if part == "foot" { "head" } else { "foot" };
                let step = if part == "foot" { 1 } else { -1 };
                let delta = facing_step(&facing)?;
                let at = [x + delta[0] * step, y, z + delta[2] * step];
                if let Some(other) = world.get(at)
                    && other.id == block.id
                    && merged(facts, &other.id, &other.properties, "part")?.as_deref()
                        == Some(partner)
                    && merged(facts, &other.id, &other.properties, "facing")? == Some(facing.clone())
                {
                    return Ok(None);
                }
                if let Some(other) = world.get(at) {
                    // `otherStates` is made whenever there is a block there.
                    facts.defaults(&other.id)?;
                }
                Ok(Some(format!(
                    "Incomplete bed at [{x}, {y}, {z}]: {}[part={part},facing={facing}] needs its {partner} at [{}, {}, {}] with the same facing (a bed faces from its foot toward its head)",
                    block.id.replacen("minecraft:", "", 1),
                    at[0],
                    at[1],
                    at[2]
                )))
            };
            match check() {
                Ok(message) => found = message,
                Err(error) => failed = Some(error),
            }
        });
        if let Some(error) = failed {
            return Err(error);
        }
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}

/// `spec({ id, states })`.
fn spec(id: &str, states: &[(String, String)]) -> String {
    let name = id.strip_prefix("minecraft:").unwrap_or(id);
    if states.is_empty() {
        return name.to_string();
    }
    let pairs: Vec<String> = states.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{name}[{}]", pairs.join(","))
}

/// `unsupportedBlocks(settled, near)`: the blocks of a settled build, in the
/// chunks `near` names, that cannot survive where they are (Minecraft's
/// `Block.canSurvive`), said with where a full block beside each would hold
/// it.
pub fn unsupported_blocks(
    settled: &SettledWorld,
    near: &KeySet,
    facts: &MinecraftFacts,
) -> Result<Option<String>, String> {
    let found: Vec<&BlockPos> = settled
        .unsupported
        .iter()
        .filter(|[x, y, z]| near.contains(&key_of(x >> 4, y >> 4, z >> 4)))
        .collect();
    if found.is_empty() {
        return Ok(None);
    }
    let mut shown = Vec::new();
    for &&[x, y, z] in found.iter().take(SHOWN_BLOCKS) {
        let block = settled
            .world
            .get([x, y, z])
            .ok_or_else(|| "Cannot read properties of null (reading 'id')".to_string())?;
        let holders: Vec<String> = facts
            .held_by(&block.id, &block.properties)?
            .iter()
            .map(|[dx, dy, dz]| format!("[{}, {}, {}]", x + dx, y + dy, z + dz))
            .collect();
        shown.push(format!(
            "{} at [{x}, {y}, {z}]: {}",
            spec(&block.id, &block.properties),
            if holders.is_empty() {
                "no full block beside it would hold it".to_string()
            } else {
                format!("a full block at {} would hold it", holders.join(" or "))
            }
        ));
    }
    let n = found.len();
    let one = n == 1;
    let more = if n > SHOWN_BLOCKS {
        format!("; and {} more", n - SHOWN_BLOCKS)
    } else {
        String::new()
    };
    Ok(Some(format!(
        "Minecraft cannot keep {} of this build where {} (Block.canSurvive: a player could not place {} there, and the first update that asks again removes {}): {}{more}",
        if one {
            "a block".to_string()
        } else {
            format!("{n} blocks")
        },
        if one { "it is" } else { "they are" },
        if one { "it" } else { "them" },
        if one { "it" } else { "one" },
        shown.join("; ")
    )))
}

fn bounds(world: &World) -> Value {
    match world.bounds() {
        None => Value::Null,
        Some(bounds) => json!({"min": bounds.min, "max": bounds.max}),
    }
}

impl Build {
    /// `executeBuild(conversation, args, blockRegistry)`.
    pub async fn run(
        &self,
        chat: &mut Chat,
        args: &Map<String, Value>,
    ) -> Result<Value, ToolError> {
        let services = &self.0;
        let program = js::string_or_empty(args.get("program"));
        if js::trim(&program).is_empty() {
            return Ok(json!({"ok": false, "error": "build tool called with an empty program"}));
        }
        let max = services.budgets.simulation_ticks;
        let settle_ticks = match args.get("settle_ticks") {
            None => settle::DEFAULT_SETTLE_TICKS,
            Some(value) => match settle::check_ticks(value, "settle_ticks", max) {
                Ok(ticks) => ticks,
                Err(error) => return Ok(failure(error, NOT_RUN)),
            },
        };
        // The build runs on a clone the runner makes of the world (`world.clone()`,
        // which throws for a world with writes since its last finalize).
        chat.world.clone_world()?;
        let start_ticks = settle::current_ticks(&chat.world);
        let settled_before = match services
            .settler
            .settle(&chat.id, &mut chat.world, &[start_ticks], Some(start_ticks))
            .await
        {
            Ok(mut settles) => settles.remove(&start_ticks).expect("the tick asked").value,
            Err(error) => {
                return Ok(failure(
                    format!("Minecraft could not settle the current world: {error}"),
                    NOT_RUN,
                ));
            }
        };
        let request = BuildRequest {
            source: program,
            registry: (*services.registry).clone(),
            read_file: true,
            settled_ticks: Some(start_ticks as u32),
        };
        let mut host = Host {
            workspace: &chat.workspace,
            settled: settled_before.world.clone(),
        };
        // A build runs to its end once started, as `runProgram` does today.
        let outcome = match services
            .runner
            .run(&request, &chat.world, &mut host, &CancellationToken::new())
            .await
        {
            Ok(outcome) => outcome,
            Err(failed) => {
                let error = if js::utf16_len(&failed.message) > MAX_ERROR_CHARS {
                    format!(
                        "{}\n[error message cut at {MAX_ERROR_CHARS} characters]",
                        js::utf16_prefix(&failed.message, MAX_ERROR_CHARS)
                    )
                } else {
                    failed.message
                };
                let mut result = Map::new();
                result.insert("ok".into(), json!(false));
                result.insert("error".into(), json!(error));
                if !failed.logs.is_empty() {
                    result.insert("logs".into(), json!(failed.logs));
                }
                result.insert("note".into(), json!(ABORTED));
                return Ok(Value::Object(result));
            }
        };
        let mut candidate = outcome.world;
        let logs = outcome.logs;
        let files = outcome.files;
        // mc.writeFile and mc.dump land in /work only now, every path
        // checked before anything is written or applied.
        for file in &files {
            if let Err(error) =
                mcbench_sandbox::work::resolve_work_path(&chat.workspace, &file.path, true)
            {
                return Ok(failure(error, ABORTED));
            }
        }
        let diff = candidate.diff(&chat.world)?;
        let changed = diff.added + diff.removed + diff.changed > 0;
        let aborted = |error: String| {
            let mut result = Map::new();
            result.insert("ok".into(), json!(false));
            result.insert("error".into(), json!(error));
            if !logs.is_empty() {
                result.insert("logs".into(), json!(logs));
            }
            result.insert("note".into(), json!(ABORTED));
            Value::Object(result)
        };
        let mut minecraft = None;
        if changed {
            let near = candidate.near_changed_chunks();
            if let Some(bed) = incomplete_bed(&candidate, &near, &services.facts)? {
                return Ok(aborted(bed));
            }
            candidate.finalize();
            let settled = match services
                .settler
                .settle(
                    &chat.id,
                    &mut candidate,
                    &[settle_ticks],
                    Some(settle_ticks),
                )
                .await
            {
                Ok(mut settles) => settles.remove(&settle_ticks).expect("the tick asked").value,
                Err(error) => {
                    return Ok(failure(
                        format!("Minecraft could not settle this build: {error}"),
                        ABORTED,
                    ));
                }
            };
            minecraft = Some(minecraft_report(&settled));
            if let Some(unsupported) = unsupported_blocks(&settled, &near, &services.facts)? {
                return Ok(aborted(unsupported));
            }
        }
        // What the build writes counts against the chat's storage budget
        // before any of it is applied: its /work files, then the chunk files
        // of its world.
        let file_bytes: u64 = files.iter().map(|file| file.text.len() as u64).sum();
        let id = chat.id.clone();
        let world = candidate.clone();
        let stored = services
            .blocking(move |store| -> Result<(), mcbench_store::Error> {
                if file_bytes > 0 {
                    store.storage().reserve(&id, file_bytes, true)?;
                }
                if changed && let Err(error) = store.persist_world(&id, &world) {
                    if file_bytes > 0 {
                        store.storage().refund(&id, file_bytes);
                    }
                    return Err(error);
                }
                Ok(())
            })
            .await;
        if let Err(error) = stored {
            return Ok(failure(error.message, ABORTED));
        }
        for file in &files {
            mcbench_sandbox::work::write_work_file(
                &chat.workspace,
                &file.path,
                file.text.as_bytes(),
            )?;
        }
        if changed {
            chat.world = candidate.clone();
        }
        let mut result = Map::new();
        result.insert("ok".into(), json!(true));
        result.insert("worldRevision".into(), json!(chat.world.revision()));
        result.insert("changed".into(), json!(changed));
        result.insert(
            "diff".into(),
            json!({"added": diff.added, "removed": diff.removed, "changed": diff.changed}),
        );
        result.insert("totalBlocks".into(), json!(candidate.size()));
        result.insert("bounds".into(), bounds(&candidate));
        if let Some(minecraft) = minecraft {
            result.insert("minecraft".into(), minecraft);
        }
        result.insert("logs".into(), json!(logs));
        if !files.is_empty() {
            let wrote: Vec<String> = files
                .iter()
                .map(|file| format!("/work/{}", js::strip_work(&file.path)))
                .collect();
            result.insert("wrote".into(), json!(wrote));
        }
        Ok(Value::Object(result))
    }
}

#[async_trait]
impl Tool for Build {
    fn name(&self) -> &'static str {
        "build"
    }

    async fn call(
        &self,
        chat: &mut Chat,
        arguments: &Map<String, Value>,
        _progress: &mut (dyn FnMut(Progress) + Send),
        _cancel: &CancellationToken,
    ) -> Result<ToolOutcome, ToolError> {
        Ok(ToolOutcome::result(self.run(chat, arguments).await?))
    }
}
