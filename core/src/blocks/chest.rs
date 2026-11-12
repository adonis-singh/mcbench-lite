// ChestBlock, which TrappedChestBlock extends (trapped_chest.rs).

use super::container::{self, Cont, Container, Kind};
use super::{diode, BlockEntity, Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    affect_neighbors_after_removal: Some(container::affect_neighbors_after_removal),
    tick: Some(tick),
    get_analog_output_signal: Some(get_analog_output_signal),
    trigger_event: Some(container::trigger_event),
    new_block_entity: Some(new_block_entity),
    // getTicker gives the lid animation's ticker on the client only.
    ticker: Some(container::no_ticker),
    ..NONE
};

// ChestBlock.TYPE.
pub fn chest_type_property(l: &Level) -> u16 {
    property!(l, "type", ["single", "left", "right"])
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChestType {
    Single,
    Left,
    Right,
}

impl ChestType {
    // ChestType.getOpposite.
    fn opposite(self) -> ChestType {
        match self {
            ChestType::Single => ChestType::Single,
            ChestType::Left => ChestType::Right,
            ChestType::Right => ChestType::Left,
        }
    }

    fn name(self) -> &'static str {
        match self {
            ChestType::Single => "single",
            ChestType::Left => "left",
            ChestType::Right => "right",
        }
    }
}

pub fn chest_type(l: &Level, state: State) -> ChestType {
    match l.reg.value_name(state, chest_type_property(l)) {
        "single" => ChestType::Single,
        "left" => ChestType::Left,
        "right" => ChestType::Right,
        other => panic!("no chest type {other}"),
    }
}

// ChestBlock.getConnectedDirection.
pub fn connected_direction(l: &Level, state: State) -> Dir {
    let direction = l.reg.get_dir(state, diode::facing(l));
    if chest_type(l, state) == ChestType::Left {
        direction.clockwise()
    } else {
        direction.counter_clockwise()
    }
}

// ChestBlock.updateShape, with chestCanConnectTo (state.is(this)).
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    let can_connect = l.block(neighbor) == l.block(state);
    update_shape_connecting(l, state, pos, direction, neighbor, can_connect)
}

// ChestBlock.updateShape for a chest class whose chestCanConnectTo gives
// `can_connect` for the neighbour.
pub fn update_shape_connecting(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor: State, can_connect: bool) -> State {
    super::shapes::common::schedule_water_tick(l, state, pos);
    if can_connect && direction.is_horizontal() {
        let neighbor_type = chest_type(l, neighbor);
        if chest_type(l, state) == ChestType::Single
            && neighbor_type != ChestType::Single
            && l.reg.get_dir(state, diode::facing(l)) == l.reg.get_dir(neighbor, diode::facing(l))
            && connected_direction(l, neighbor) == direction.opposite()
        {
            return l.reg.with_name(state, chest_type_property(l), neighbor_type.opposite().name());
        }
    } else if connected_direction(l, state) == direction {
        return l.reg.with_name(state, chest_type_property(l), ChestType::Single.name());
    }
    state
}

// ChestBlock.isChestBlockedAt: a conductor above (an entity lookup for a
// sitting cat finds none).
fn is_chest_blocked_at(l: &mut Level, pos: Pos) -> bool {
    let above = l.get(pos.above());
    l.is_conductor(above)
}

// BlockEntityType.getBlockEntity for the chest block's type: the block
// entity at the position (Level.getBlockEntity makes it if missing), if it is
// of the type.
fn entity_of_type(l: &mut Level, pos: Pos, trapped: bool) -> bool {
    match l.block_entity(pos) {
        Some(BlockEntity::Container(Container { kind: Kind::Chest, .. })) => !trapped,
        Some(BlockEntity::Container(Container { kind: Kind::TrappedChest, .. })) => trapped,
        Some(_) | None => false,
    }
}

// ChestBlock.getContainer(chest, state, level, pos, override), by
// DoubleBlockCombiner.combineWithNeigbour: the combined container (a
// CompoundContainer of the first and second chest for a double chest), or
// none for acceptNone.
pub fn get_container(l: &mut Level, state: State, pos: Pos, override_blocked: bool) -> Option<Cont> {
    let trapped = l.reg.blocks[l.block(state) as usize].class == "TrappedChestBlock";
    if !entity_of_type(l, pos, trapped) {
        return None;
    }
    if !override_blocked && is_chest_blocked_at(l, pos) {
        return None;
    }
    let block_type = chest_type(l, state);
    if block_type == ChestType::Single {
        return Some(Cont::Be(pos));
    }
    let neighbor_pos = pos.relative(connected_direction(l, state));
    let Some(neighbor) = l.get_if_loaded(neighbor_pos) else { return Some(Cont::Be(pos)) };
    if l.block(neighbor) == l.block(state) {
        let neighbor_type = chest_type(l, neighbor);
        if neighbor_type != ChestType::Single && block_type != neighbor_type && l.reg.get_dir(neighbor, diode::facing(l)) == l.reg.get_dir(state, diode::facing(l)) {
            if !override_blocked && is_chest_blocked_at(l, neighbor_pos) {
                return None;
            }
            if entity_of_type(l, neighbor_pos, trapped) {
                // The first of the pair is the right chest.
                return Some(if block_type == ChestType::Right { Cont::Double(pos, neighbor_pos) } else { Cont::Double(neighbor_pos, pos) });
            }
        }
    }
    Some(Cont::Be(pos))
}

// ChestBlock.getAnalogOutputSignal.
fn get_analog_output_signal(l: &mut Level, state: State, pos: Pos, _direction: Dir) -> i32 {
    match get_container(l, state, pos, false) {
        Some(container) => container::redstone_signal(l, &container),
        None => 0,
    }
}

// ChestBlock.tick: ChestBlockEntity.recheckOpen, which is
// ContainerOpenersCounter.recheckOpeners. No entity holds the chest open
// (none exists), so the count stays 0, and openerCountChanged(0, 0) runs:
// ChestBlockEntity.signalOpenCount posts the open count as a block event
// (TrappedChestBlockEntity's neighbour updates need the two to differ).
pub fn tick(l: &mut Level, state: State, pos: Pos) {
    if matches!(l.block_entity(pos), Some(BlockEntity::Container(Container { kind: Kind::Chest | Kind::TrappedChest, .. }))) {
        l.block_event(pos, l.block(state), 1, 0);
    }
}

// ChestBlock.newBlockEntity.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Container(Container::new(Kind::Chest))
}
