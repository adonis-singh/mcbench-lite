// The cases scripts/movement-parity.js gives the port and the Java client
// (scripts/MovementReference.java): a world of blocks set without any update,
// a player, and the ticks to run; each tick's state is printed with every
// number as its exact bits.

use super::{Input, Keys, Player, Settings};
use crate::blocks::Blocks;
use crate::dir::Pos;
use crate::level::Level;
use crate::phys::Vec3;
use crate::registry::Registry;

fn put(level: &mut Level, pos: Pos, state: u16) {
    // (A position outside the build height holds void air whatever is set there.)
    if crate::world::World::outside(pos) {
        return;
    }
    level.chunk(pos.x() >> 4, pos.z() >> 4);
    level.world.set(pos, state);
}

fn dump(player: &Player, tick: i32) -> String {
    let (p, v) = (player.pos, player.delta);
    let d = |x: f64| format!("{:x}", x.to_bits());
    let f = |x: f32| format!("{:x}", x.to_bits());
    let (xxa, zza, jump_trigger, sprint_trigger, no_jump, auto_jump) = player.timers();
    let (water_height, lava_height) = player.fluid_heights();
    let (eye_water, eye_lava) = player.fluids_on_eyes();
    format!(
        "t{} p={},{},{} v={},{},{} og={} fl={} pose={} sp={} cr={} hc={} vc={} fd={} in={},{} tr={},{},{},{} sw={} wt={} fh={},{} ey={}{} sb={} mc={} vb={} sm={},{} ib={} dim={},{},{}",
        tick,
        d(p.x),
        d(p.y),
        d(p.z),
        d(v.x),
        d(v.y),
        d(v.z),
        player.on_ground as i32,
        player.flying as i32,
        player.pose.name(),
        player.is_sprinting() as i32,
        player.is_crouching() as i32,
        player.horizontal_collision as i32,
        player.vertical_collision as i32,
        d(player.fall_distance),
        f(xxa),
        f(zza),
        jump_trigger,
        sprint_trigger,
        no_jump,
        auto_jump,
        player.is_swimming() as i32,
        player.is_touching_water() as i32,
        d(water_height),
        d(lava_height),
        eye_water as i32,
        eye_lava as i32,
        player.supporting_block().map_or("-".to_string(), |b| format!("{}:{}:{}", b.x(), b.y(), b.z())),
        player.minor_horizontal_collision() as i32,
        player.vertical_collision_below as i32,
        d(player.stuck().x),
        d(player.stuck().y),
        player.in_block().map_or("-".to_string(), |s| s.to_string()),
        f(player.width()),
        f(player.height()),
        f(player.eye_height()),
    )
}

// Runs the cases of a file and returns what scripts/MovementReference.java
// prints for the same file, but for a case the port refuses ("unported:"),
// which prints "refused <name> <reason>".
pub fn run(reg: &'static Registry, blocks: &'static Blocks, text: &str) -> String {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let mut out = String::new();
    let mut case: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.starts_with("case ") {
            case.clear();
        }
        case.push(line);
        if line == "end" {
            let name = case[0][5..].to_string();
            match catch_unwind(AssertUnwindSafe(|| run_case(reg, blocks, &case))) {
                Ok(printed) => out.push_str(&printed),
                Err(payload) => match payload.downcast::<crate::level::Refusal>() {
                    Ok(refusal) => out.push_str(&format!("refused {name} unported: {}\n", refusal.0)),
                    Err(payload) => {
                        let text = payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                        out.push_str(&format!("error {name} {}\n", text.replace('\n', " ")));
                    }
                },
            }
        }
    }
    out
}

fn run_case(reg: &'static Registry, blocks: &'static Blocks, lines: &[&str]) -> String {
    let mut out = String::new();
    let mut level: Option<Level> = None;
    let mut tick = 0;
    let mut pistons = false;
    for line in lines {
        let p: Vec<&str> = line.split(' ').collect();
        match p[0] {
            "case" => {
                out.push_str(&format!("case {}\n", p[1]));
                level = Some(Level::new(reg, blocks, 0, false));
                pistons = false;
            }
            "set" | "edit" => {
                let n = |i: usize| p[i].parse::<i32>().unwrap();
                let (state, _) = reg.parse_state(p[4]);
                put(level.as_mut().unwrap(), Pos::new(n(1), n(2), n(3)), state);
            }
            "editplayer" => {
                let n = |i: usize| p[i].parse::<i32>().unwrap();
                let (state, _) = reg.parse_state(p[4]);
                let at = level.as_ref().unwrap().player.as_ref().unwrap().block_position();
                put(level.as_mut().unwrap(), Pos::new(at.x() + n(1), at.y() + n(2), at.z() + n(3)), state);
            }
            "fill" => {
                let n = |i: usize| p[i].parse::<i32>().unwrap();
                let (state, _) = reg.parse_state(p[7]);
                let level = level.as_mut().unwrap();
                for z in n(3)..=n(6) {
                    for y in n(2)..=n(5) {
                        for x in n(1)..=n(4) {
                            put(level, Pos::new(x, y, z), state);
                        }
                    }
                }
            }
            "boat" => {
                // A boat standing still, which the player collides with.
                let level = level.as_mut().unwrap();
                let f = |i: usize| p[i].parse::<f64>().unwrap();
                let mut boat = crate::entity::boat::new_boat(level, "minecraft:oak_boat");
                crate::entity::motion::set_pos(level, &mut boat, f(1), f(2), f(3));
                crate::entity::add_fresh_entity(level, boat);
            }
            "moving" => {
                // A moving piston (block and block entity) at its progress 0.
                let level = level.as_mut().unwrap();
                let n = |i: usize| p[i].parse::<i32>().unwrap();
                let pos = Pos::new(n(1), n(2), n(3));
                let (block, _) = reg.parse_state(p[4]);
                let (moved, _) = reg.parse_state(p[5]);
                put(level, pos, block);
                level.ticket(pos.x() >> 4, pos.z() >> 4);
                let moving = crate::blocks::moving_piston::Moving::new(moved, crate::dir::Dir::by_name(p[6]), p[7] == "1", p[8] == "1");
                level.set_block_entity(pos, crate::blocks::BlockEntity::Piston(moving));
                pistons = true;
            }
            "player" => {
                let f = |i: usize| p[i].parse::<f64>().unwrap();
                let settings = Settings { auto_jump: p.len() > 7 && p[7] == "1" };
                let made = Player::new(Vec3::new(f(1), f(2), f(3)), p[4].parse().unwrap(), p[5].parse().unwrap(), p[6] == "1", settings);
                out.push_str(&dump(&made, -1));
                out.push('\n');
                level.as_mut().unwrap().player = Some(Box::new(made));
                tick = 0;
            }
            "tick" => {
                let input = Input { keys: Keys::from_bits(p[1].parse().unwrap()), yaw: p[2].parse().unwrap(), pitch: p[3].parse().unwrap() };
                let level = level.as_mut().unwrap();
                // The client ticks its entities, then its block entities, then its time.
                level.player_tick(input);
                if pistons {
                    level.tick_block_entities();
                }
                level.game_time += 1;
                out.push_str(&dump(level.player.as_ref().unwrap(), tick));
                out.push('\n');
                tick += 1;
            }
            "end" => out.push_str("done\n"),
            other => panic!("bad line {other}"),
        }
    }
    out
}
