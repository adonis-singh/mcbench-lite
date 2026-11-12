// mcbench-core --probe: answers lines of a few questions about saved data
// (SNBT, item stacks, block entities, entities), in the form that the
// parity scripts' Java probe answers them, so the two can be compared on
// many inputs (scripts/core-probe-parity.js). An input this port does not
// model answers "REFUSE <what>".

use crate::codec::Fail;
use crate::components;
use crate::nbt;
use crate::registry::Registry;

pub fn run(reg: &'static Registry, blocks: &'static crate::blocks::Blocks) {
    use std::io::{BufRead, Write};
    let mut out = std::io::stdout();
    // The lines share one level ("be" and "mpshape" place blocks on it), a
    // failed one's half-done work and all, as McbenchProbe's do.
    let mut level = crate::level::Level::new(reg, blocks, 0, false);
    level.ticket(0, 0);
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let (cmd, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle(reg, &mut level, cmd, rest)));
        let text = match answer {
            Ok(text) => text,
            Err(payload) => match payload.downcast::<crate::level::Refusal>() {
                Ok(refusal) => format!("REFUSE {}", refusal.0),
                Err(payload) => {
                    let text = payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                    format!("PANIC {}", text.replace('\n', " "))
                }
            },
        };
        writeln!(out, "A {text}").unwrap();
        out.flush().unwrap();
    }
}

fn refuse(fail: Fail) -> String {
    format!("REFUSE {}", fail.0)
}

fn parse_compound(text: &str) -> Result<nbt::Compound, String> {
    nbt::parse_compound(text).map_err(|e| format!("REFUSE {e:?}"))
}

fn handle(reg: &'static Registry, level: &mut crate::level::Level, cmd: &str, rest: &str) -> String {
    match cmd {
        "snbt" => match parse_compound(rest) {
            Ok(tag) => format!("OK {tag}"),
            Err(refusal) => refusal,
        },
        "value" => match nbt::parse(rest) {
            Ok(tag) => format!("OK {tag}"),
            Err(e) => format!("REFUSE {e:?}"),
        },
        "stack" => {
            let tag = match parse_compound(rest) {
                Ok(tag) => tag,
                Err(refusal) => return refusal,
            };
            match components::decode_stack(reg, &nbt::Tag::Compound(tag)) {
                Ok(stack) => match components::encode_stack(reg, &stack) {
                    Ok(encoded) => format!("OK {encoded} | patch={} max={}", stack.patch.is_empty(), stack.max_stack_size(reg)),
                    Err(fail) => refuse(fail),
                },
                Err(fail) => refuse(fail),
            }
        }
        "be" => {
            // be x y z block snbt: the block placed, its block entity loaded.
            let p: Vec<&str> = rest.splitn(5, ' ').collect();
            let pos = crate::dir::Pos::new(p[0].parse().unwrap(), p[1].parse().unwrap(), p[2].parse().unwrap());
            crate::engine::place_commands(level, &[format!("setblock {} {} {} {}", p[0], p[1], p[2], p[3])], true);
            let Some(entity) = level.block_entity(pos) else { return "NOBE".into() };
            let mut tag = match parse_compound(p[4]) {
                Ok(tag) => tag,
                Err(refusal) => return refusal,
            };
            let id = entity.type_key(reg);
            if !tag.contains("id") {
                tag.put_string("id", id);
            }
            tag.put_int("x", pos.x());
            tag.put_int("y", pos.y());
            tag.put_int("z", pos.z());
            let entity = level.block_entities.get_mut(&pos).unwrap();
            match crate::blocks::load::load(reg, entity, &tag) {
                Ok(extras) => level.set_block_entity_extras(pos, extras),
                Err(fail) => return refuse(fail),
            }
            crate::blocks::loaded(level, pos);
            // The entity that was loaded is saved after setChanged has told the
            // neighbours (Java holds the object, though what they do may break its
            // block; one broken is saved as it was loaded).
            let loaded = format!("OK {} || {}", level.block_entity_saved_with_metadata(pos), level.block_entity_saved(pos).1);
            crate::blocks::container::be_changed(level, pos);
            if level.block_entities.contains_key(&pos) {
                format!("OK {} || {}", level.block_entity_saved_with_metadata(pos), level.block_entity_saved(pos).1)
            } else {
                loaded
            }
        }
        "entity" => {
            // A level of its own: a refused load leaves other draws behind.
            *level = crate::level::Level::new(reg, level.blocks, 0, false);
            level.ticket(0, 0);
            // EntityType.loadEntityRecursive, tryAddFreshEntityWithPassengers
            // and saveAsPassenger of an item entity.
            let tag = match parse_compound(rest) {
                Ok(tag) => tag,
                Err(refusal) => return refusal,
            };
            let id = tag.get("id").and_then(|t| t.as_str()).unwrap_or("").to_string();
            if !matches!(id.as_str(), "minecraft:item" | "minecraft:item_frame" | "minecraft:glow_item_frame") {
                return "REFUSE an entity that is not an item or an item frame".into();
            }
            let loaded = match crate::entity::load::load_entity(level, &id, &tag) {
                Ok(loaded) => loaded,
                Err(fail) => return refuse(fail),
            };
            let uuid = loaded.entity.uuid;
            let duplicate = level.entities.all.values().any(|e| e.uuid == uuid);
            let (id, position, removed) = (loaded.entity.id, loaded.entity.pos, loaded.entity.removed);
            let save = |level: &crate::level::Level, e: &crate::entity::Entity, origin| match crate::entity::load::save_entity(level, e, origin, loaded.spawn_reason) {
                Ok(Some(saved)) => saved.to_string(),
                Ok(None) => "{}".into(),
                Err(fail) => format!("REFUSE {}", fail.0),
            };
            if duplicate || removed {
                // Not added: the entity still saves (a removed one does not).
                let text = save(level, &loaded.entity, loaded.origin);
                return if text.starts_with("REFUSE") { text } else { format!("OK added={} {text}", !duplicate) };
            }
            crate::entity::add_fresh_entity(level, loaded.entity);
            let mut origin = loaded.origin;
            if origin.is_none() && level.entities.accessible.ids().contains(&id) {
                origin = Some((position, level.world_uuid));
            }
            let text = save(level, &level.entities.all[&id], origin);
            if text.starts_with("REFUSE") {
                text
            } else {
                format!("OK added=true {text}")
            }
        }
        "mpshape" => moving_piston_shape(reg, level, rest),
        other => format!("UNKNOWN {other}"),
    }
}

// A shape as McbenchEngine.shapeLine writes it (without the "shape" field):
// the coordinates per axis, the voxel sizes, the voxels, the single box.
fn shape_line(shape: &crate::registry::Shape) -> String {
    use crate::jfmt::double_to_string as d;
    let mut line = String::new();
    for axis in 0..3 {
        line.push_str(&shape.coords[axis].iter().map(|c| d(*c)).collect::<Vec<_>>().join(","));
        line.push('\t');
    }
    line.push_str(&format!("{},{},{}\t", shape.size[0], shape.size[1], shape.size[2]));
    for x in 0..shape.size[0] {
        for y in 0..shape.size[1] {
            for z in 0..shape.size[2] {
                line.push(if shape.is_full(x, y, z) { '1' } else { '0' });
            }
        }
    }
    line.push('\t');
    match shape.single {
        Some(b) => line.push_str(&b.iter().map(|c| d(*c)).collect::<Vec<_>>().join(",")),
        None => line.push('-'),
    }
    line
}

// mpshape <moved state> <direction> <extending 0|1> <source 0|1> <progress> <noclip direction or ->:
// a moving piston's collision shape as the block reads it from the level,
// its boxes, its faces and the sturdiness of each, and whether it is a full block.
fn moving_piston_shape(reg: &'static Registry, level: &mut crate::level::Level, rest: &str) -> String {
    use crate::blocks::moving_piston::{self, Moving};
    use crate::dir::Dir;
    use crate::jfmt::double_to_string as d;
    use crate::registry::Support;
    let p: Vec<&str> = rest.split(' ').collect();
    let pos = if p.len() > 6 { crate::dir::Pos::new(p[6].parse().unwrap(), 70, p[7].parse().unwrap()) } else { crate::dir::Pos::new(3, 70, 3) };
    let (moved, _) = reg.parse_state(p[0]);
    let moving = Moving::new(moved, Dir::by_name(p[1]), p[2] == "1", p[3] == "1").at_progress(p[4].parse().unwrap());
    let block = level.reg.default_state(reg.block_key("minecraft:moving_piston"));
    level.set(pos, block, 0);
    level.set_block_entity(pos, crate::blocks::BlockEntity::Piston(moving));
    let noclip = (p[5] != "-").then(|| Dir::by_name(p[5]));
    let state = level.get(pos);
    let shape = moving_piston::with_noclip(noclip, || crate::shape_ops::collision_shape_at(level, state, pos).into_owned());
    let mut out = format!("OK {}", shape_line(&shape));
    out.push_str(" | boxes");
    if !shape.is_empty() {
        for b in shape.boxes() {
            out.push(' ');
            out.push_str(&b.iter().map(|c| d(*c)).collect::<Vec<_>>().join(","));
        }
    }
    for direction in crate::dir::ALL {
        out.push_str(&format!(" | {} {}", direction.name(), shape_line(&shape.face_shape(direction))));
        for support in [Support::Full, Support::Center, Support::Rigid] {
            out.push(if moving_piston::with_noclip(noclip, || crate::shape_ops::is_face_sturdy(level, state, pos, direction, support)) { '1' } else { '0' });
        }
    }
    out.push_str(&format!(" | full {}", shape.is_full_block()));
    level.set(pos, level.known.air, 0);
    out
}
