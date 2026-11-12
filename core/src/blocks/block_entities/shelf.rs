// ShelfBlock and ShelfBlockEntity, with SideChainPartBlock's code: a
// powered shelf joins the powered shelves beside it that face the same way
// into a chain of at most three (the side_chain property), and a shelf
// that loses power or is removed leaves its chain. The shelf's block entity
// is a Container of three slots (ShelfBlockEntity, a ListBackedContainer):
// hoppers, droppers and crafters fill and empty it, and its analog output
// tells which slots hold an item.

use super::trigger_event;
use crate::blocks::entities::{Entity, ItemShelf};
use crate::blocks::shapes::common::{facing, get_facing, powered, schedule_water_tick};
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, UPDATE_ALL};
use crate::property;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(neighbor_changed),
    on_place: Some(on_place),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    get_analog_output_signal: Some(get_analog_output_signal),
    trigger_event: Some(trigger_event),
    new_block_entity: Some(new_block_entity),
    ..NONE
};

// SideChainPartBlock.getMaxChainLength.
const MAX_CHAIN_LENGTH: i32 = 3;

// SideChainPart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Unconnected,
    Right,
    Center,
    Left,
}

impl Part {
    fn name(self) -> &'static str {
        match self {
            Part::Unconnected => "unconnected",
            Part::Right => "right",
            Part::Center => "center",
            Part::Left => "left",
        }
    }

    fn by_name(name: &str) -> Part {
        match name {
            "unconnected" => Part::Unconnected,
            "right" => Part::Right,
            "center" => Part::Center,
            "left" => Part::Left,
            other => panic!("no side chain part {other}"),
        }
    }

    fn is_connected(self) -> bool {
        self != Part::Unconnected
    }

    fn is_connection_towards(self, part: Part) -> bool {
        self == Part::Center || self == part
    }

    fn is_chain_end(self) -> bool {
        self != Part::Center
    }

    fn when_connected_to_the_right(self) -> Part {
        match self {
            Part::Unconnected | Part::Left => Part::Left,
            Part::Right | Part::Center => Part::Center,
        }
    }

    fn when_connected_to_the_left(self) -> Part {
        match self {
            Part::Unconnected | Part::Right => Part::Right,
            Part::Center | Part::Left => Part::Center,
        }
    }

    fn when_disconnected_from_the_right(self) -> Part {
        match self {
            Part::Unconnected | Part::Left => Part::Unconnected,
            Part::Right | Part::Center => Part::Right,
        }
    }

    fn when_disconnected_from_the_left(self) -> Part {
        match self {
            Part::Unconnected | Part::Right => Part::Unconnected,
            Part::Center | Part::Left => Part::Left,
        }
    }
}

// ShelfBlock.SIDE_CHAIN_PART.
fn side_chain_property(l: &Level) -> u16 {
    property!(l, "side_chain", ["unconnected", "right", "center", "left"])
}

// ShelfBlock.getSideChainPart and setSideChainPart.
fn side_chain_part(l: &Level, state: State) -> Part {
    Part::by_name(l.reg.value_name(state, side_chain_property(l)))
}

fn set_side_chain_part(l: &Level, state: State, part: Part) -> State {
    l.reg.with_name(state, side_chain_property(l), part.name())
}

// ShelfBlock.isConnectable: a powered state of a wooden shelf.
fn is_connectable(l: &Level, state: State) -> bool {
    l.reg.in_tag(state, "minecraft:wooden_shelves") && l.reg.get_bool(state, powered(l))
}

// SideChainPartBlock.Neighbor: an EmptyNeighbor has no part, a
// SideChainNeighbor has the part it had when it was made.
#[derive(Clone, Copy)]
struct Neighbor {
    pos: Pos,
    part: Option<Part>,
}

impl Neighbor {
    fn is_connectable(self) -> bool {
        self.part.is_some()
    }

    fn is_unconnectable_or_chain_end(self) -> bool {
        self.part.map_or(true, Part::is_chain_end)
    }

    fn connects_towards(self, part: Part) -> bool {
        self.part.is_some_and(|p| p.is_connection_towards(part))
    }

    // SideChainNeighbor.connectToTheRight and the others: SideChainPartBlock.setPart
    // with the part cached at the neighbour's creation.
    fn connect_to_the_right(self, l: &mut Level) {
        if let Some(part) = self.part {
            set_part(l, self.pos, part.when_connected_to_the_right());
        }
    }

    fn connect_to_the_left(self, l: &mut Level) {
        if let Some(part) = self.part {
            set_part(l, self.pos, part.when_connected_to_the_left());
        }
    }

    fn disconnect_from_the_right(self, l: &mut Level) {
        if let Some(part) = self.part {
            set_part(l, self.pos, part.when_disconnected_from_the_right());
        }
    }

    fn disconnect_from_the_left(self, l: &mut Level) {
        if let Some(part) = self.part {
            set_part(l, self.pos, part.when_disconnected_from_the_left());
        }
    }
}

// SideChainPartBlock.Neighbors: the neighbours made so far by position (its
// cache), so a neighbour keeps the part it was first read with.
struct Neighbors {
    facing: Dir,
    center: Pos,
    cache: Vec<Neighbor>,
}

impl Neighbors {
    fn new(facing: Dir, center: Pos) -> Neighbors {
        Neighbors { facing, center, cache: Vec::new() }
    }

    // getOrCreateNeighbor.
    fn get(&mut self, l: &mut Level, direction: Dir, distance: i32) -> Neighbor {
        let pos = self.center.relative_n(direction, distance);
        if let Some(&neighbor) = self.cache.iter().find(|n| n.pos == pos) {
            return neighbor;
        }
        // createNewNeighbor.
        let state = l.get(pos);
        let connectable = is_connectable(l, state) && get_facing(l, state) == self.facing;
        let neighbor = Neighbor { pos, part: connectable.then(|| side_chain_part(l, state)) };
        self.cache.push(neighbor);
        neighbor
    }

    fn left(&mut self, l: &mut Level, distance: i32) -> Neighbor {
        self.get(l, self.facing.clockwise(), distance)
    }

    fn right(&mut self, l: &mut Level, distance: i32) -> Neighbor {
        self.get(l, self.facing.counter_clockwise(), distance)
    }
}

// SideChainPartBlock.setPart.
fn set_part(l: &mut Level, pos: Pos, part: Part) {
    let state = l.get(pos);
    if side_chain_part(l, state) != part {
        l.set(pos, set_side_chain_part(l, state, part), UPDATE_ALL);
    }
}

// SideChainPartBlock.getAllBlocksConnectedTo: how many positions it lists
// (the position itself and the connected ones either side, none if the
// position is not connectable).
fn count_connected_to(l: &mut Level, pos: Pos) -> i32 {
    let state = l.get(pos);
    if !is_connectable(l, state) {
        return 0;
    }
    let mut neighbors = Neighbors::new(get_facing(l, state), pos);
    let mut count = 1;
    // addBlocksConnectingTowards, left then right.
    for (towards, left) in [(Part::Left, true), (Part::Right, false)] {
        for distance in 1..MAX_CHAIN_LENGTH {
            let neighbor = if left { neighbors.left(l, distance) } else { neighbors.right(l, distance) };
            if neighbor.connects_towards(towards) {
                count += 1;
            }
            if neighbor.is_unconnectable_or_chain_end() {
                break;
            }
        }
    }
    count
}

// SideChainPartBlock.updateNeighborsAfterPoweringDown.
fn update_neighbors_after_powering_down(l: &mut Level, pos: Pos, state: State) {
    let mut neighbors = Neighbors::new(get_facing(l, state), pos);
    neighbors.left(l, 1).disconnect_from_the_right(l);
    neighbors.right(l, 1).disconnect_from_the_left(l);
}

// SideChainPartBlock.canConnect.
fn can_connect(segment_length: i32, current_chain_length: i32) -> bool {
    segment_length > 0 && current_chain_length + segment_length <= MAX_CHAIN_LENGTH
}

// SideChainPartBlock.updateSelfAndNeighborsOnPoweringUp.
fn update_self_and_neighbors_on_powering_up(l: &mut Level, pos: Pos, state: State, old: State) {
    // isBeingUpdatedByNeighbor.
    let being_updated_by_neighbor = side_chain_part(l, state).is_connected() || (is_connectable(l, old) && side_chain_part(l, old).is_connected());
    if !is_connectable(l, state) || being_updated_by_neighbor {
        return;
    }
    let mut neighbors = Neighbors::new(get_facing(l, state), pos);
    let mut part = Part::Unconnected;
    let left = neighbors.left(l, 1);
    let i = if left.is_connectable() { count_connected_to(l, left.pos) } else { 0 };
    let right = neighbors.right(l, 1);
    let i1 = if right.is_connectable() { count_connected_to(l, right.pos) } else { 0 };
    let mut i2 = 1;
    if can_connect(i, i2) {
        part = part.when_connected_to_the_left();
        neighbors.left(l, 1).connect_to_the_right(l);
        i2 += i;
    }
    if can_connect(i1, i2) {
        part = part.when_connected_to_the_right();
        neighbors.right(l, 1).connect_to_the_left(l);
    }
    set_part(l, pos, part);
}

// ShelfBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    if l.reg.get_bool(state, powered(l)) {
        update_self_and_neighbors_on_powering_up(l, pos, state, old);
    } else {
        update_neighbors_after_powering_down(l, pos, state);
    }
}

// ShelfBlock.affectNeighborsAfterRemoval: Containers.updateNeighboursAfterDestroy,
// then the chain forgets the shelf.
fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, _moved: bool) {
    l.update_neighbour_for_output_signal(pos, l.block(state));
    update_neighbors_after_powering_down(l, pos, state);
}

// ShelfBlock.neighborChanged. The redstone change event has no listener:
// the new current stands. The shelf's sound draws nothing from the level's
// random; its game event is block_activate or block_deactivate, with the new
// state as context.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor_block: BlockId, _moved: bool) {
    let has_neighbor_signal = l.has_neighbor_signal(pos);
    if l.reg.get_bool(state, powered(l)) != has_neighbor_signal {
        let mut shaped = l.reg.with_bool(state, powered(l), has_neighbor_signal);
        if !has_neighbor_signal {
            shaped = set_side_chain_part(l, shaped, Part::Unconnected);
        }
        l.set(pos, shaped, UPDATE_ALL);
        let event = if has_neighbor_signal { crate::game_event!(l, "block_activate") } else { crate::game_event!(l, "block_deactivate") };
        l.game_event_in(event, pos, crate::game_event::Context::of_state(shaped));
    }
}

// ShelfBlock.updateShape: a waterlogged shelf schedules a water tick;
// BlockBehaviour.updateShape keeps the state.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    state
}

// ShelfBlock.getAnalogOutputSignal: from the facing side only, a bit for
// each slot that holds an item (Level.getBlockEntity makes the entity if
// missing).
fn get_analog_output_signal(l: &mut Level, state: State, pos: Pos, direction: Dir) -> i32 {
    if direction != l.reg.get_dir(state, facing(l)).opposite() {
        return 0;
    }
    match l.block_entity(pos) {
        Some(BlockEntity::Entity(Entity::Shelf(shelf))) => {
            shelf.items.iter().enumerate().fold(0, |bits, (slot, item)| bits | ((!crate::blocks::container::is_empty(item)) as i32) << slot)
        }
        _ => 0,
    }
}

// ShelfBlock.newBlockEntity.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Entity(Entity::Shelf(ItemShelf::new()))
}

// ShelfBlockEntity.setChanged(): setChanged(GameEvent.BLOCK_ACTIVATE).
// BlockEntity.setChanged tells the comparators unless updates are ignored
// (Paper's hopper transfers); the game event with the entity's state goes out
// either way, and sendBlockUpdated reaches no player or mob.
pub fn set_changed(l: &mut Level, pos: Pos) {
    crate::blocks::container::be_changed(l, pos);
    let state = l.get(pos);
    let event = crate::game_event!(l, "block_activate");
    l.game_event_in(event, pos, crate::game_event::Context::of_state(state));
}
