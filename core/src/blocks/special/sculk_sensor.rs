// SculkSensorBlock and CalibratedSculkSensorBlock, with their block
// entities (SculkSensorBlockEntity, CalibratedSculkSensorBlockEntity): a
// vibration listener that, on a vibration, powers the block for 30 ticks
// (10 for the calibrated one) and gives off a signal.

use super::vibration::{self, Data};
use super::{Special, UserKind};
use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos};
use crate::entity::Entity;
use crate::game_event::{Context, GameEvent, Source};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL, UPDATE_CLIENTS, UPDATE_KNOWN_SHAPE};
use crate::phys;
use crate::property;
use crate::registry::{Registry, State};

pub const SENSOR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    tick: Some(tick),
    on_place: Some(on_place),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    get_signal: Some(get_signal),
    get_direct_signal: Some(get_direct_signal),
    get_analog_output_signal: Some(get_analog_output_signal),
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    step_on: Some(step_on),
    ..NONE
};

// CalibratedSculkSensorBlock: a facing side that gives no signal, the
// calibrated entity and its short active time.
pub const CALIBRATED: Behaviour = Behaviour { get_signal: Some(calibrated_get_signal), new_block_entity: Some(new_calibrated_block_entity), ..SENSOR };

// SculkSensorBlockEntity: the vibration data and the frequency of the last
// vibration it received.
#[derive(Clone, Debug)]
pub struct Sensor {
    pub vibration: Data,
    pub last_vibration_frequency: i32,
    pub calibrated: bool,
}

impl Sensor {
    // The entity's type key and saved data (keys in order).
    pub fn saved(&self, reg: &Registry) -> (&'static str, String) {
        let key = if self.calibrated { "minecraft:calibrated_sculk_sensor" } else { "minecraft:sculk_sensor" };
        (key, format!("{{components:{{}},last_vibration_frequency:{},{}}}", self.last_vibration_frequency, self.vibration.saved(reg)))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Inactive,
    Active,
    Cooldown,
}

fn phase_property(l: &Level) -> u16 {
    property!(l, "sculk_sensor_phase", ["inactive", "active", "cooldown"])
}

fn power_property(l: &Level) -> u16 {
    property!(l, "power", ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15"])
}

fn waterlogged_property(l: &Level) -> u16 {
    property!(l, "waterlogged", ["true", "false"])
}

fn facing_property(l: &Level) -> u16 {
    property!(l, "facing", ["north", "south", "west", "east"])
}

// SculkSensorBlock.getPhase.
pub fn phase(l: &Level, state: State) -> Phase {
    match l.reg.value_name(state, phase_property(l)) {
        "inactive" => Phase::Inactive,
        "active" => Phase::Active,
        "cooldown" => Phase::Cooldown,
        other => panic!("no sculk sensor phase {other}"),
    }
}

fn with_phase(l: &Level, state: State, phase: Phase) -> State {
    l.reg.with_name(
        state,
        phase_property(l),
        match phase {
            Phase::Inactive => "inactive",
            Phase::Active => "active",
            Phase::Cooldown => "cooldown",
        },
    )
}

// instanceof SculkSensorBlock (the calibrated sensor is one).
pub fn is_sculk_sensor(l: &Level, state: State) -> bool {
    crate::shape_classes!(l, ["SculkSensorBlock", "CalibratedSculkSensorBlock"])[l.block(state) as usize]
}

fn is_calibrated(l: &Level, state: State) -> bool {
    l.reg.blocks[l.block(state) as usize].class == "CalibratedSculkSensorBlock"
}

// getActiveTicks.
fn active_ticks(l: &Level, state: State) -> i32 {
    if is_calibrated(l, state) {
        10
    } else {
        30
    }
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::SculkSensor(Sensor { vibration: Data::default(), last_vibration_frequency: 0, calibrated: false }))
}

fn new_calibrated_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::SculkSensor(Sensor { vibration: Data::default(), last_vibration_frequency: 0, calibrated: true }))
}

// getTicker on the server: VibrationSystem.Ticker.tick.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    fn tick_entity(l: &mut Level, pos: Pos, _state: State) {
        vibration::tick(l, pos);
    }
    Some(tick_entity)
}

// updateShape: a waterlogged sensor schedules a water tick.
fn update_shape(l: &mut Level, state: State, pos: Pos, _dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    state
}

// SculkSensorBlock.tick. The redstone events have no listener.
fn tick(l: &mut Level, state: State, pos: Pos) {
    match phase(l, state) {
        Phase::Active => deactivate(l, pos, state),
        Phase::Cooldown => {
            l.set(pos, with_phase(l, state, Phase::Inactive), UPDATE_ALL);
            if !l.reg.get_bool(state, waterlogged_property(l)) {
                // playSound's pitch.
                l.random.next_float();
            }
        }
        Phase::Inactive => {}
    }
}

// SculkSensorBlock.deactivate.
fn deactivate(l: &mut Level, pos: Pos, state: State) {
    let off = l.reg.with_int(with_phase(l, state, Phase::Cooldown), power_property(l), 0);
    l.set(pos, off, UPDATE_ALL);
    l.schedule_tick(pos, l.block(state), 10, PRIORITY_NORMAL);
    update_neighbours(l, pos, state);
}

// SculkSensorBlock.stepOn: an entity stepping on an idle sensor is a step
// vibration at the entity's position, forced past the listener's checks.
fn step_on(l: &mut Level, pos: Pos, state: State, entity: &Entity) {
    if !can_activate(l, state) {
        return;
    }
    // level.getBlockEntity(pos) makes the block entity if it is missing.
    if !matches!(l.block_entity(pos), Some(BlockEntity::Special(Special::SculkSensor(_)))) {
        return;
    }
    let step = crate::game_event!(l, "step");
    let kind = if is_calibrated(l, state) { UserKind::Calibrated } else { UserKind::Sensor };
    if vibration::can_receive_vibration(l, kind, pos, pos, step, Context::of_state(state)) {
        // EntityInteractEvent has no listener.
        let source = Source::of(l, entity);
        vibration::force_schedule_vibration(l, pos, step, Context::of_entity(Some(source)), entity.pos);
    }
}

// SculkSensorBlock.canActivate.
fn can_activate(l: &Level, state: State) -> bool {
    vibration::can_activate(l, state)
}

// SculkSensorBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    let block = l.block(state);
    if block != l.block(old) && l.reg.get_int(state, power_property(l)) > 0 && !l.has_scheduled_tick(pos, block) {
        l.set(pos, l.reg.with_int(state, power_property(l), 0), UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE);
    }
}

// SculkSensorBlock.affectNeighborsAfterRemoval.
fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, _moved: bool) {
    if phase(l, state) == Phase::Active {
        update_neighbours(l, pos, state);
    }
}

// SculkSensorBlock.updateNeighbours.
fn update_neighbours(l: &mut Level, pos: Pos, state: State) {
    let block = l.block(state);
    l.update_neighbors_at(pos, block);
    l.update_neighbors_at(pos.below(), block);
}

fn get_signal(l: &mut Level, state: State, _pos: Pos, _dir: Dir) -> i32 {
    l.reg.get_int(state, power_property(l))
}

// CalibratedSculkSensorBlock.getSignal.
fn calibrated_get_signal(l: &mut Level, state: State, pos: Pos, dir: Dir) -> i32 {
    if dir != l.reg.get_dir(state, facing_property(l)) {
        get_signal(l, state, pos, dir)
    } else {
        0
    }
}

// getDirectSignal: state.getSignal, which is this class's getSignal.
fn get_direct_signal(l: &mut Level, state: State, pos: Pos, dir: Dir) -> i32 {
    if dir == Dir::Up {
        crate::blocks::get_signal(l, state, pos, dir)
    } else {
        0
    }
}

// SculkSensorBlock.getAnalogOutputSignal.
fn get_analog_output_signal(l: &mut Level, state: State, pos: Pos, _dir: Dir) -> i32 {
    let frequency = match l.block_entity(pos) {
        Some(BlockEntity::Special(Special::SculkSensor(sensor))) => sensor.last_vibration_frequency,
        _ => return 0,
    };
    if phase(l, state) == Phase::Active {
        frequency
    } else {
        0
    }
}

// SculkSensorBlockEntity.VibrationUser.onReceiveVibration.
pub fn on_receive_vibration(l: &mut Level, listener: Pos, event: GameEvent, entity: Option<Source>, distance: f32) {
    let state = l.get(listener);
    if !can_activate(l, state) {
        return;
    }
    let frequency = vibration::frequency(l.reg, event);
    if let Some(BlockEntity::Special(Special::SculkSensor(sensor))) = l.block_entities.get_mut(&listener) {
        sensor.last_vibration_frequency = frequency;
    }
    let strength = redstone_strength_for_distance(distance, vibration::listener_radius(l, listener));
    activate(l, entity, listener, state, strength, frequency);
}

// VibrationSystem.getRedstoneStrengthForDistance.
fn redstone_strength_for_distance(distance: f32, max_distance: i32) -> i32 {
    let d = 15.0 / max_distance as f64;
    1.max(15 - phys::floor(d * distance as f64))
}

// SculkSensorBlock.activate. The redstone event (BlockRedstoneEvent) has no
// listener: the power stands, and it is always above 0.
fn activate(l: &mut Level, entity: Option<Source>, pos: Pos, state: State, power: i32, frequency: i32) {
    let active = l.reg.with_int(with_phase(l, state, Phase::Active), power_property(l), power);
    l.set(pos, active, UPDATE_ALL);
    let (block, ticks) = (l.block(state), active_ticks(l, state));
    l.schedule_tick(pos, block, ticks, PRIORITY_NORMAL);
    update_neighbours(l, pos, state);
    try_resonate_vibration(l, entity, pos, frequency);
    let clicking = crate::game_event!(l, "sculk_sensor_tendrils_clicking");
    l.game_event_at(entity, clicking, pos);
    if !l.reg.get_bool(state, waterlogged_property(l)) {
        // playSound's pitch.
        l.random.next_float();
    }
}

// SculkSensorBlock.tryResonateVibration: each neighbour that resonates
// (amethyst) sends the resonance event for the frequency.
fn try_resonate_vibration(l: &mut Level, entity: Option<Source>, pos: Pos, frequency: i32) {
    for dir in crate::dir::ALL {
        let at = pos.relative(dir);
        let state = l.get(at);
        if l.reg.in_tag(state, "minecraft:vibration_resonators") {
            let event = l.reg.game_event_by_key[&format!("minecraft:resonate_{frequency}")];
            l.game_event_in(event, at, Context::of(entity, state));
        }
    }
}
