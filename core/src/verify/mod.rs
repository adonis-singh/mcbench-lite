// McbenchEngine.verify: a verifier experiment (McbenchExperiment) as the
// engine runs it. A fresh level holding the request's columns as plugin
// chunk tickets, the import's commands placed in the request's mode, then
// server ticks until the experiment ends. report.json and result.json are
// the Java engine's, byte for byte but for the engine's name and the
// timings (see experiment.rs for what is refused).
//
// A seeded request makes the Java engine add the level's saved chunks (blocks,
// light, heightmaps: Mojang's SerializableChunkData) to result.json; this
// port writes them (chunk_save.rs).

mod experiment;
mod gson;

use crate::blocks::Blocks;
use crate::datafix;
use crate::engine;
use crate::level::{refuse, Level};
use crate::nbt::{Compound, Tag};
use crate::registry::Registry;
use experiment::Experiment;
pub use experiment::DATA_VERSION;
use std::path::Path;

// A VERIFY request. Ok is the Java engine's MCBENCH_VERIFIED: the experiment
// ran to its end, and result.json's ok says how it ended (an action, a
// budget or an observation that failed ends it there, as
// McbenchExperiment.endOfTick does; the host reads result.json for that).
// Err is the reason the Java engine would reply MCBENCH_VERIFY_FAILED with:
// the request could not run, or its level failed (result.json then records
// that too).
pub fn verify(reg: &'static Registry, blocks: &'static Blocks, request_file: &Path, commands_file: &Path, folder: &Path) -> Result<(), String> {
    let request = gson::parse(&std::fs::read_to_string(request_file).expect("read the request"));
    // The plugin's chunk tickets, not the live import's.
    let seed = if request.has("seed") { request.req("seed").long_value() } else { 0 };
    let mut level = Level::new(reg, blocks, seed, false);
    for column in request.req("columns").arr() {
        let column = column.arr();
        level.ticket(column[0].int_value(), column[1].int_value());
    }
    std::fs::create_dir_all(folder).expect("create the output folder");
    let mut experiment = Experiment::new(request, folder)?;
    experiment.check_ported();
    let request = experiment.request().clone();
    let mut lines: Vec<String> = std::fs::read_to_string(commands_file).expect("read the commands").lines().map(str::to_string).collect();
    let mut migrations: Vec<(String, String)> = Vec::new();
    if request.has("block_data_version") {
        let version = request.req("block_data_version").int_value();
        for line in &mut lines {
            let mut parts: Vec<String> = line.split(' ').map(str::to_string).collect();
            let index = if parts[0] == "setblock" { 4 } else { 7 };
            parts[index] = upgrade_block_token(&parts[index], version, &mut migrations)?;
            *line = parts.join(" ");
        }
    }
    // The server places with no check for an error Minecraft reported: the
    // first tick fails with it.
    engine::place_commands(&mut level, &lines, request.req("placementMode").str() == "strict");
    experiment.start(&mut level, &migrations)?;
    experiment.begin(&level);
    // A request that asks for it gets the entities' state after every tick, in
    // entity-trace.txt (for comparing engines).
    let trace = request.has("entity_trace") && request.req("entity_trace").bool();
    let trace_file = folder.join("entity-trace.txt");
    if trace {
        let _ = std::fs::remove_file(&trace_file);
        write_trace(&level, &trace_file);
    }
    while experiment.running {
        level.tick();
        if let Err(cause) = level.check_reported() {
            let error = "java.lang.IllegalStateException: The engine's level failed";
            experiment.finish(&mut level, Some(error.into()));
            return Err(format!("{error} <- {cause}"));
        }
        if let Err(error) = experiment.end_of_tick(&mut level) {
            experiment.finish(&mut level, Some(error));
        }
        if trace {
            write_trace(&level, &trace_file);
        }
    }
    Ok(())
}

// What the entity trace holds after a tick (McbenchEngine.trace): the game
// time, the level random, the entity id counter, the shared random, each
// entity's state lines and the saved data of the vehicles, paintings and
// armor stands.
fn write_trace(level: &Level, file: &Path) {
    use std::io::Write;
    let mut out = String::new();
    out.push_str(&format!("time {}\nrandom {}\nentityid {}\nsharedrandom {}\n", level.game_time, level.random.state(), level.entities.counter, level.shared_random.state()));
    out.push_str(&crate::entity::state_text(level));
    // The end gateways: age and teleport cooldown, by position (as the Java
    // engine orders them: by BlockPos.asLong).
    let mut gateways: Vec<(&crate::dir::Pos, &crate::blocks::BlockEntity)> = level.block_entities.iter().filter(|(_, be)| matches!(be, crate::blocks::BlockEntity::Special(crate::blocks::special::Special::EndGateway(_)))).collect();
    gateways.sort_by_key(|(pos, _)| pos.as_long());
    for (pos, be) in gateways {
        if let crate::blocks::BlockEntity::Special(crate::blocks::special::Special::EndGateway(gateway)) = be {
            out.push_str(&format!("gateway {} {} {} {}\n", pos.x(), pos.y(), pos.z(), gateway.state_text()));
        }
    }
    for event in &level.block_events {
        out.push_str(&format!("blockevent {} {} {} {} {}\n", event.pos.x(), event.pos.y(), event.pos.z(), event.a, event.b));
    }
    for &id in level.entities.accessible.ids() {
        let e = &level.entities.all[&id];
        if !crate::entity::saves_in_trace(&e.kind) {
            continue;
        }
        out.push_str(&format!("save {id} "));
        match crate::entity::load::save_entity(level, e, e.origin, e.spawn_reason) {
            Ok(Some(saved)) => out.push_str(&saved.to_string()),
            Ok(None) => out.push('-'),
            Err(fail) => refuse(format!("saving an entity for the trace: {}", fail.0)),
        }
        out.push('\n');
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(file).expect("open the entity trace");
    f.write_all(out.as_bytes()).expect("write the entity trace");
}

// McbenchExperiment.upgradeBlockToken: the block's name and properties as a
// block state tag, upgraded by the data fixer from the request's block data
// version, and written back with its full name and its properties by name.
fn upgrade_block_token(token: &str, version: i32, migrations: &mut Vec<(String, String)>) -> Result<String, String> {
    if let Some((_, after)) = migrations.iter().find(|(before, _)| before == token) {
        return Ok(after.clone());
    }
    let bracket = token.find('[');
    let name = bracket.map_or(token, |at| &token[..at]);
    let Ok(name) = crate::codec::resource_location(name) else { refuse("a block token whose name is no resource location (ResourceLocationException)") };
    let mut tag = Compound::new();
    tag.put_string("Name", &name);
    if let Some(at) = bracket {
        if !token.ends_with(']') {
            return Err("java.io.IOException: Invalid source block properties".into());
        }
        let mut properties = Compound::new();
        for pair in java_split_commas(&token[at + 1..token.len() - 1]) {
            let kv: Vec<&str> = pair.split('=').collect();
            if kv.len() != 2 || properties.contains(kv[0]) {
                return Err("java.io.IOException: Invalid source block property".into());
            }
            properties.put_string(kv[0], kv[1]);
        }
        tag.put("Properties", Tag::Compound(properties));
    }
    let tag = datafix::update_compound(datafix::BLOCK_STATE, tag, version);
    let Some(Tag::String(name)) = tag.get("Name") else { return Err("java.io.IOException: Native block data fixer omitted Name".into()) };
    let mut result = name.clone();
    if let Some(Tag::Compound(properties)) = tag.get("Properties") {
        if !properties.is_empty() {
            let mut keys: Vec<&String> = properties.0.iter().map(|(key, _)| key).collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            let mut pairs = Vec::new();
            for key in keys {
                let Some(Tag::String(value)) = properties.get(key) else { return Err("java.io.IOException: Invalid upgraded property".into()) };
                pairs.push(format!("{key}={value}"));
            }
            result.push_str(&format!("[{}]", pairs.join(",")));
        }
    }
    migrations.push((token.to_string(), result.clone()));
    Ok(result)
}

// String.split(","): trailing empty strings are dropped, but an empty
// string splits into itself.
fn java_split_commas(text: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = text.split(',').collect();
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_token_is_normalized_like_the_data_fixer_leaves_it() {
        let mut migrations = Vec::new();
        let now = DATA_VERSION as i32;
        assert_eq!(upgrade_block_token("lever[powered=false,face=wall]", now, &mut migrations).unwrap(), "minecraft:lever[face=wall,powered=false]");
        assert_eq!(upgrade_block_token("minecraft:stone", now, &mut migrations).unwrap(), "minecraft:stone");
        assert_eq!(migrations.len(), 2);
        assert!(upgrade_block_token("lever[face=wall,face=floor]", now, &mut migrations).is_err());
        assert!(upgrade_block_token("lever[face]", now, &mut migrations).is_err());
        assert!(upgrade_block_token("lever[face=wall", now, &mut migrations).is_err());
    }

    #[test]
    fn a_block_token_of_an_older_data_version_is_upgraded() {
        let mut migrations = Vec::new();
        assert_eq!(upgrade_block_token("chain[axis=y,waterlogged=false]", 4189, &mut migrations).unwrap(), "minecraft:iron_chain[axis=y,waterlogged=false]");
        assert_eq!(upgrade_block_token("grass", 3465, &mut migrations).unwrap(), "minecraft:short_grass");
    }
}
