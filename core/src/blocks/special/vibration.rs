// VibrationSystem: a vibration listener's data (the vibration on its way
// and the candidates it chose from), its Listener (GameEventListener), its
// Ticker, and the Users the sculk sensors and the sculk shrieker are.
// Entities that could change a vibration's fate (players, wardens,
// projectiles) do not exist here.

use super::{Special, UserKind};
use crate::blocks::{BlockEntity, special};
use crate::dir::{Dir, Pos};
use crate::game_event::{center_of, Context, GameEvent, Source};
use crate::jfmt::{double_to_string, float_to_string};
use crate::level::Level;
use crate::phys::{self, Vec3};
use crate::registry::Registry;

// VibrationInfo: the game event, how far it was from the listener, where it
// happened and the entity that caused it.
#[derive(Clone, Debug)]
pub struct Info {
    pub event: GameEvent,
    pub distance: f32,
    pub pos: Vec3,
    pub source: Option<Source>,
}

// VibrationSystem.Data (the particle reload flag only draws particles).
#[derive(Clone, Debug, Default)]
pub struct Data {
    pub current: Option<Info>,
    pub travel_time: i32,
    // VibrationSelector.currentVibrationData: the candidate and its tick.
    pub selector: Option<(Info, i64)>,
}

// VibrationSystem.getGameEventFrequency.
pub fn frequency(reg: &Registry, event: GameEvent) -> i32 {
    reg.game_events[event as usize].frequency
}

// Whether the game event is in the tag.
pub fn event_in_tag(reg: &Registry, event: GameEvent, tag: &str) -> bool {
    reg.game_events[event as usize].tags.iter().any(|t| t == tag)
}

impl Data {
    // VibrationSystem.Data.CODEC read from the `listener` tag of a saved entity
    // (a listener that fails to decode is only logged by Minecraft). Only a
    // quiet listener is read: a vibration on its way or a candidate refuses.
    pub fn load(input: &crate::codec::Input) -> crate::codec::R<Data> {
        use crate::codec::{fail, long, map, required, Input};
        let mut data = Data::default();
        let Some(tag) = input.get("listener") else { return Ok(data) };
        let listener = Input(map(tag)?);
        if listener.get("event").is_some() {
            return fail("a vibration listener's saved vibration");
        }
        // (event_delay is a non-negative int: a negative one makes the whole
        // listener a decode error, which leaves the entity's own default.)
        let delay = listener.int_or("event_delay", 0)?;
        if let Some(selector) = listener.get("selector") {
            let selector = map(selector)?;
            if selector.get("event").is_some() {
                return fail("a vibration listener's saved candidate");
            }
            long(required(selector, "tick")?)?;
        }
        if delay >= 0 {
            data.travel_time = delay;
        }
        Ok(data)
    }

    // VibrationSelector.addCandidate, with shouldReplaceVibration.
    pub fn add_candidate(&mut self, reg: &Registry, info: Info, tick: i64) {
        let replace = match &self.selector {
            None => true,
            Some((current, right)) => {
                if tick != *right {
                    false
                } else {
                    info.distance < current.distance || !(info.distance > current.distance) && frequency(reg, info.event) > frequency(reg, current.event)
                }
            }
        };
        if replace {
            self.selector = Some((info, tick));
        }
    }

    // VibrationSelector.chosenCandidate.
    fn chosen_candidate(&self, tick: i64) -> Option<Info> {
        match &self.selector {
            Some((info, right)) if *right < tick => Some(info.clone()),
            _ => None,
        }
    }

    // VibrationSystem.Data.CODEC as SNBT with keys in order: the listener
    // tag a block entity stores.
    pub fn saved(&self, reg: &Registry) -> String {
        let mut out = String::from("listener:{");
        if let Some(info) = &self.current {
            out.push_str(&format!("event:{},", info_saved(reg, info)));
        }
        out.push_str(&format!("event_delay:{},selector:{{", self.travel_time));
        if let Some((info, _)) = &self.selector {
            out.push_str(&format!("event:{},", info_saved(reg, info)));
        }
        out.push_str(&format!("tick:{}L}}}}", self.selector.as_ref().map_or(-1, |(_, tick)| *tick)));
        out
    }
}

// VibrationInfo.CODEC as SNBT with keys in order.
fn info_saved(reg: &Registry, info: &Info) -> String {
    let mut out = format!(
        "{{distance:{}f,game_event:\"{}\",pos:[{}d,{}d,{}d]",
        float_to_string(info.distance),
        reg.game_events[info.event as usize].key,
        double_to_string(info.pos.x),
        double_to_string(info.pos.y),
        double_to_string(info.pos.z)
    );
    if let Some(source) = &info.source {
        // UUIDUtil.CODEC: the uuid as four ints.
        let (most, least) = source.uuid;
        out.push_str(&format!(",source:[I;{},{},{},{}]", (most >> 32) as i32, most as i32, (least >> 32) as i32, least as i32));
    }
    out.push('}');
    out
}

fn data(l: &mut Level, pos: Pos) -> &mut Data {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Special(Special::SculkSensor(sensor))) => &mut sensor.vibration,
        Some(BlockEntity::Special(Special::SculkShrieker(shrieker))) => &mut shrieker.vibration,
        _ => panic!("no vibration listener at {pos}"),
    }
}

fn user(l: &Level, pos: Pos) -> UserKind {
    match l.block_entities.get(&pos) {
        Some(BlockEntity::Special(Special::SculkSensor(sensor))) => {
            if sensor.calibrated {
                UserKind::Calibrated
            } else {
                UserKind::Sensor
            }
        }
        Some(BlockEntity::Special(Special::SculkShrieker(_))) => UserKind::Shrieker,
        _ => panic!("no vibration listener at {pos}"),
    }
}

// User.getListenerRadius: the range a sensor listens in (its Paper range
// override is never set).
pub fn listener_radius(l: &Level, pos: Pos) -> i32 {
    match user(l, pos) {
        UserKind::Sensor | UserKind::Shrieker => 8,
        UserKind::Calibrated => 16,
    }
}

// VibrationSystem.Listener.handleGameEvent.
pub fn handle_game_event(l: &mut Level, listener: Pos, event: GameEvent, context: Context, pos: Vec3) -> bool {
    let kind = user(l, listener);
    if data(l, listener).current.is_some() {
        return false;
    }
    if !is_valid_vibration(l, kind, event, context) {
        return false;
    }
    let sensor = center_of(listener);
    // The event Paper calls takes the user's answer as its default, which
    // nothing changes.
    if !can_receive_vibration(l, kind, listener, Pos::new(phys::floor(pos.x), phys::floor(pos.y), phys::floor(pos.z)), event, context) {
        return false;
    }
    if is_occluded(l, pos, sensor) {
        return false;
    }
    schedule_vibration(l, listener, event, context, pos, sensor);
    true
}

// Listener.forceScheduleVibration.
pub fn force_schedule_vibration(l: &mut Level, listener: Pos, event: GameEvent, context: Context, pos: Vec3) {
    let sensor = center_of(listener);
    schedule_vibration(l, listener, event, context, pos, sensor);
}

// Listener.scheduleVibration.
fn schedule_vibration(l: &mut Level, listener: Pos, event: GameEvent, context: Context, pos: Vec3, sensor: Vec3) {
    let info = Info { event, distance: pos.distance_to_sqr(sensor).sqrt() as f32, pos, source: context.entity };
    let (reg, tick) = (l.reg, l.game_time);
    data(l, listener).add_candidate(reg, info, tick);
}

// User.isValidVibration. Item entities are never spectators and never
// sneak.
fn is_valid_vibration(l: &Level, kind: UserKind, event: GameEvent, context: Context) -> bool {
    let tag = match kind {
        UserKind::Sensor | UserKind::Calibrated => "minecraft:vibrations",
        UserKind::Shrieker => "minecraft:shrieker_can_listen",
    };
    if !event_in_tag(l.reg, event, tag) {
        return false;
    }
    if let Some(entity) = context.entity {
        if entity.dampens_vibrations {
            return false;
        }
    }
    context.state.map_or(true, |state| !l.reg.in_tag(state, "minecraft:dampens_vibrations"))
}

// User.canReceiveVibration of the sensors and the shrieker. A shrieker
// answers only to a player (tryGetPlayer: a ServerPlayer in the shrieker's
// level, or what one controls, owns or threw): the input player is the only
// one there is.
pub fn can_receive_vibration(l: &mut Level, kind: UserKind, listener: Pos, event_pos: Pos, event: GameEvent, context: Context) -> bool {
    match kind {
        UserKind::Sensor => sensor_can_receive(l, listener, event_pos, event),
        UserKind::Calibrated => {
            let state = l.get(listener);
            let opposite = l.reg.get_dir(state, l.reg.property_of(state, "facing")).opposite();
            // getBackSignal.
            let back_signal = l.signal(listener.relative(opposite), opposite);
            (back_signal == 0 || frequency(l.reg, event) == back_signal) && sensor_can_receive(l, listener, event_pos, event)
        }
        UserKind::Shrieker => {
            let state = l.get(listener);
            let shrieking = l.reg.get_bool(state, l.reg.property_of(state, "shrieking"));
            !shrieking && context.entity.is_some_and(|source| source.is_input_player())
        }
    }
}

// SculkSensorBlockEntity.VibrationUser.canReceiveVibration.
fn sensor_can_receive(l: &mut Level, listener: Pos, event_pos: Pos, event: GameEvent) -> bool {
    let destroyed_or_placed = event == crate::game_event!(l, "block_destroy") || event == crate::game_event!(l, "block_place");
    let state = l.get(listener);
    (event_pos != listener || !destroyed_or_placed) && frequency(l.reg, event) != 0 && can_activate(l, state)
}

// SculkSensorBlock.canActivate.
pub fn can_activate(l: &Level, state: crate::registry::State) -> bool {
    super::sculk_sensor::is_sculk_sensor(l, state) && super::sculk_sensor::phase(l, state) == super::sculk_sensor::Phase::Inactive
}

// VibrationSystem.Listener.isOccluded: whether every one of the six lines
// from just outside the event's block to the listener's block meets a block
// that occludes vibrations.
fn is_occluded(l: &mut Level, event_pos: Vec3, sensor: Vec3) -> bool {
    let from = Vec3::new(phys::floor(event_pos.x) as f64 + 0.5, phys::floor(event_pos.y) as f64 + 0.5, phys::floor(event_pos.z) as f64 + 0.5);
    let to = Vec3::new(phys::floor(sensor.x) as f64 + 0.5, phys::floor(sensor.y) as f64 + 0.5, phys::floor(sensor.z) as f64 + 0.5);
    for dir in crate::dir::ALL {
        // Vec3.relative(direction, 1.0E-5F).
        let (dx, dy, dz) = dir.offset();
        let length = 1.0E-5f32 as f64;
        let start = Vec3::new(from.x + length * dx as f64, from.y + length * dy as f64, from.z + length * dz as f64);
        if !block_in_line(l, start, to) {
            return false;
        }
    }
    true
}

// BlockGetter.isBlockInLine with the occluding blocks as the target: whether
// the line from `from` to `to` passes through one (type BLOCK, not MISS).
fn block_in_line(l: &mut Level, from: Vec3, to: Vec3) -> bool {
    traverse_blocks(l, from, to, &mut |l, pos| {
        let state = l.get(pos);
        l.reg.in_tag(state, "minecraft:occludes_vibration_signals")
    })
}

// BlockGetter.traverseBlocks: the blocks a line passes through, until the
// tester accepts one.
pub fn traverse_blocks(l: &mut Level, from: Vec3, to: Vec3, tester: &mut dyn FnMut(&mut Level, Pos) -> bool) -> bool {
    if from == to {
        return false;
    }
    let d = phys::lerp(-1.0E-7, to.x, from.x);
    let d1 = phys::lerp(-1.0E-7, to.y, from.y);
    let d2 = phys::lerp(-1.0E-7, to.z, from.z);
    let d3 = phys::lerp(-1.0E-7, from.x, to.x);
    let d4 = phys::lerp(-1.0E-7, from.y, to.y);
    let d5 = phys::lerp(-1.0E-7, from.z, to.z);
    let mut x = phys::floor(d3);
    let mut y = phys::floor(d4);
    let mut z = phys::floor(d5);
    if tester(l, Pos::new(x, y, z)) {
        return true;
    }
    let d6 = d - d3;
    let d7 = d1 - d4;
    let d8 = d2 - d5;
    let i = phys::sign(d6);
    let i1 = phys::sign(d7);
    let i2 = phys::sign(d8);
    let d9 = if i == 0 { f64::MAX } else { i as f64 / d6 };
    let d10 = if i1 == 0 { f64::MAX } else { i1 as f64 / d7 };
    let d11 = if i2 == 0 { f64::MAX } else { i2 as f64 / d8 };
    let mut d12 = d9 * if i > 0 { 1.0 - phys::frac(d3) } else { phys::frac(d3) };
    let mut d13 = d10 * if i1 > 0 { 1.0 - phys::frac(d4) } else { phys::frac(d4) };
    let mut d14 = d11 * if i2 > 0 { 1.0 - phys::frac(d5) } else { phys::frac(d5) };
    while d12 <= 1.0 || d13 <= 1.0 || d14 <= 1.0 {
        if d12 < d13 {
            if d12 < d14 {
                x += i;
                d12 += d9;
            } else {
                z += i2;
                d14 += d11;
            }
        } else if d13 < d14 {
            y += i1;
            d13 += d10;
        } else {
            z += i2;
            d14 += d11;
        }
        if tester(l, Pos::new(x, y, z)) {
            return true;
        }
    }
    false
}

// VibrationSystem.Ticker.tick, for the listener at `pos`.
pub fn tick(l: &mut Level, pos: Pos) {
    let kind = user(l, pos);
    if data(l, pos).current.is_none() {
        // trySelectAndScheduleVibration: the vibration travels floor(distance)
        // ticks (User.calculateTravelTimeInTicks); the particle goes to the
        // players near it, none.
        let tick = l.game_time;
        let d = data(l, pos);
        if let Some(info) = d.chosen_candidate(tick) {
            d.travel_time = phys::floor(info.distance as f64);
            d.current = Some(info);
            super::set_changed(l, pos);
            data(l, pos).selector = None;
        }
    }
    if let Some(info) = data(l, pos).current.clone() {
        let d = data(l, pos);
        let mut changed = d.travel_time > 0;
        // decrementTravelTime.
        d.travel_time = (d.travel_time - 1).max(0);
        if d.travel_time <= 0 {
            changed = receive_vibration(l, kind, pos, &info);
        }
        if changed {
            super::set_changed(l, pos);
        }
    }
}

// VibrationSystem.Ticker.receiveVibration: whether the vibration arrived.
fn receive_vibration(l: &mut Level, kind: UserKind, listener: Pos, info: &Info) -> bool {
    let event_pos = Pos::new(phys::floor(info.pos.x), phys::floor(info.pos.y), phys::floor(info.pos.z));
    // requiresAdjacentChunksToBeTicking: the sensors and the shrieker both.
    for cx in (listener.x() >> 4) - 1..=(listener.x() >> 4) + 1 {
        for cz in (listener.z() >> 4) - 1..=(listener.z() >> 4) + 1 {
            if !l.ticks_in_chunk(cx, cz) || !l.world.has_chunk(cx, cz) {
                return false;
            }
        }
    }
    // Listener.distanceBetweenInBlocks.
    let (dx, dy, dz) = ((event_pos.x() - listener.x()) as f64, (event_pos.y() - listener.y()) as f64, (event_pos.z() - listener.z()) as f64);
    let distance = (dx * dx + dy * dy + dz * dz).sqrt() as f32;
    let generation = l.block_entity_generation(listener);
    match kind {
        UserKind::Sensor | UserKind::Calibrated => special::sculk_sensor::on_receive_vibration(l, listener, info.event, info.source, distance),
        UserKind::Shrieker => special::sculk_shrieker::on_receive_vibration(l, listener, info.source),
    }
    // The vibration is cleared on the data object that received it, which a
    // block entity replaced meanwhile no longer owns.
    if l.block_entity_generation(listener) == generation {
        data(l, listener).current = None;
    }
    true
}

// Direction is unused here.
#[allow(dead_code)]
type D = Dir;
