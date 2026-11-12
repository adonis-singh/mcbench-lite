// BaseRailBlock and its classes (RailBlock, PoweredRailBlock for the powered
// and the activator rail, DetectorRailBlock) with RailState, on Paper's
// defaults: the Bukkit events they call (BlockRedstoneEvent,
// EntityInsideBlockEvent) have no listener, so their results stand. Fluids
// are not modelled: a waterlogged rail refuses in updateShape, which
// schedules its water tick.
//
// Minecarts come from imports (entity/minecart.rs); the detector rail finds
// them with the level's minecart search, and reads the signal of a container
// cart on it.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos, HORIZONTAL};
use crate::level::{refuse, Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::phys::Aabb;
use crate::property;
use crate::registry::{BlockId, State, Support};

pub const RAIL: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(neighbor_changed::<RAIL_KIND>),
    on_place: Some(on_place::<RAIL_KIND>),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    can_survive: Some(can_survive),
    ..NONE
};

pub const POWERED: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(neighbor_changed::<POWERED_KIND>),
    on_place: Some(on_place::<POWERED_KIND>),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    can_survive: Some(can_survive),
    ..NONE
};

pub const DETECTOR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(neighbor_changed::<DETECTOR_KIND>),
    on_place: Some(on_place::<DETECTOR_KIND>),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    tick: Some(detector_tick),
    get_signal: Some(detector_get_signal),
    get_direct_signal: Some(detector_get_direct_signal),
    get_analog_output_signal: Some(detector_get_analog_output_signal),
    can_survive: Some(can_survive),
    entity_inside: Some(detector_entity_inside),
    all_entities: true,
    ..NONE
};

// The class a generic function is for.
const RAIL_KIND: u8 = 0;
const POWERED_KIND: u8 = 1;
const DETECTOR_KIND: u8 = 2;

// RailShape, in its declaration order (the order of its property's values).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RailShape {
    NorthSouth,
    EastWest,
    AscendingEast,
    AscendingWest,
    AscendingNorth,
    AscendingSouth,
    SouthEast,
    SouthWest,
    NorthWest,
    NorthEast,
}

use RailShape::*;

const SHAPES: [RailShape; 10] = [NorthSouth, EastWest, AscendingEast, AscendingWest, AscendingNorth, AscendingSouth, SouthEast, SouthWest, NorthWest, NorthEast];

impl RailShape {
    pub fn is_slope(self) -> bool {
        matches!(self, AscendingNorth | AscendingEast | AscendingSouth | AscendingWest)
    }
}

// RailBlock.SHAPE (BlockStateProperties.RAIL_SHAPE).
fn rail_shape_property(l: &Level) -> u16 {
    property!(l, "shape", ["north_south", "east_west", "ascending_east", "ascending_west", "ascending_north", "ascending_south", "south_east", "south_west", "north_west", "north_east"])
}

// PoweredRailBlock.SHAPE and DetectorRailBlock.SHAPE (RAIL_SHAPE_STRAIGHT).
fn straight_shape_property(l: &Level) -> u16 {
    property!(l, "shape", ["north_south", "east_west", "ascending_east", "ascending_west", "ascending_north", "ascending_south"])
}

// BaseRailBlock.getShapeProperty.
fn shape_property(l: &Level, state: State) -> u16 {
    let rail = rail_shape_property(l);
    if l.reg.has(state, rail) {
        rail
    } else {
        straight_shape_property(l)
    }
}

pub fn shape(l: &Level, state: State) -> RailShape {
    SHAPES[l.reg.get(state, shape_property(l, state)) as usize]
}

fn with_shape(l: &Level, state: State, shape: RailShape) -> State {
    l.reg.with(state, shape_property(l, state), shape as u16)
}

// PoweredRailBlock.POWERED, DetectorRailBlock.POWERED.
fn powered_property(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

pub fn powered(l: &Level, state: State) -> bool {
    l.reg.get_bool(state, powered_property(l))
}

// BaseRailBlock.isRail(state): a class check (the rails tag is exactly them).
fn is_rail(l: &Level, state: State) -> bool {
    super::is_rail(l, state)
}

fn is_rail_at(l: &mut Level, pos: Pos) -> bool {
    let state = l.get(pos);
    is_rail(l, state)
}

// BaseRailBlock.isStraight: every rail but the plain one.
fn is_straight(l: &Level, state: State) -> bool {
    !l.reg.has(state, rail_shape_property(l))
}

// Block.canSupportRigidBlock.
fn can_support_rigid_block(l: &mut Level, pos: Pos) -> bool {
    let state = l.get(pos);
    crate::shape_ops::is_face_sturdy(l, state, pos, Dir::Up, Support::Rigid)
}

// --- RailState ---

struct RailState {
    pos: Pos,
    block: BlockId,
    state: State,
    straight: bool,
    connections: Vec<Pos>,
}

impl RailState {
    fn new(l: &Level, pos: Pos, state: State) -> RailState {
        let mut rail = RailState { pos, block: l.block(state), state, straight: is_straight(l, state), connections: Vec::new() };
        rail.update_connections(shape(l, state));
        rail
    }

    fn is_valid(&self, l: &mut Level) -> bool {
        let state = l.get(self.pos);
        l.block(state) == self.block
    }

    fn update_connections(&mut self, shape: RailShape) {
        let pos = self.pos;
        let (north, south, west, east) = (pos.relative(Dir::North), pos.relative(Dir::South), pos.relative(Dir::West), pos.relative(Dir::East));
        self.connections = match shape {
            NorthSouth => vec![north, south],
            EastWest => vec![west, east],
            AscendingEast => vec![west, east.above()],
            AscendingWest => vec![west.above(), east],
            AscendingNorth => vec![north.above(), south],
            AscendingSouth => vec![north, south.above()],
            SouthEast => vec![east, south],
            SouthWest => vec![west, south],
            NorthWest => vec![west, north],
            NorthEast => vec![east, north],
        };
    }

    fn remove_soft_connections(&mut self, l: &mut Level) {
        let mut i = 0;
        while i < self.connections.len() {
            match get_rail(l, self.connections[i]) {
                Some(rail) if rail.connects_to(self) => {
                    self.connections[i] = rail.pos;
                    i += 1;
                }
                _ => {
                    self.connections.remove(i);
                }
            }
        }
    }

    fn connects_to(&self, other: &RailState) -> bool {
        self.has_connection(other.pos)
    }

    // Whether a connection is in the column of the position.
    fn has_connection(&self, pos: Pos) -> bool {
        self.connections.iter().any(|c| c.x() == pos.x() && c.z() == pos.z())
    }

    fn count_potential_connections(&self, l: &mut Level) -> i32 {
        let mut i = 0;
        for dir in HORIZONTAL {
            if has_rail(l, self.pos.relative(dir)) {
                i += 1;
            }
        }
        i
    }

    fn can_connect_to(&self, other: &RailState) -> bool {
        self.connects_to(other) || self.connections.len() != 2
    }

    fn connect_to(&mut self, l: &mut Level, other: &RailState) {
        if self.is_valid(l) && other.is_valid(l) {
            self.connections.push(other.pos);
            let north = self.pos.relative(Dir::North);
            let south = self.pos.relative(Dir::South);
            let west = self.pos.relative(Dir::West);
            let east = self.pos.relative(Dir::East);
            let has_north = self.has_connection(north);
            let has_south = self.has_connection(south);
            let has_west = self.has_connection(west);
            let has_east = self.has_connection(east);
            let mut rail_shape = None;
            if has_north || has_south {
                rail_shape = Some(NorthSouth);
            }
            if has_west || has_east {
                rail_shape = Some(EastWest);
            }
            if !self.straight {
                if has_south && has_east && !has_north && !has_west {
                    rail_shape = Some(SouthEast);
                }
                if has_south && has_west && !has_north && !has_east {
                    rail_shape = Some(SouthWest);
                }
                if has_north && has_west && !has_south && !has_east {
                    rail_shape = Some(NorthWest);
                }
                if has_north && has_east && !has_south && !has_west {
                    rail_shape = Some(NorthEast);
                }
            }
            if rail_shape == Some(NorthSouth) {
                if is_rail_at(l, north.above()) {
                    rail_shape = Some(AscendingNorth);
                }
                if is_rail_at(l, south.above()) {
                    rail_shape = Some(AscendingSouth);
                }
            }
            if rail_shape == Some(EastWest) {
                if is_rail_at(l, east.above()) {
                    rail_shape = Some(AscendingEast);
                }
                if is_rail_at(l, west.above()) {
                    rail_shape = Some(AscendingWest);
                }
            }
            let rail_shape = rail_shape.unwrap_or(NorthSouth);
            self.state = with_shape(l, self.state, rail_shape);
            l.set(self.pos, self.state, UPDATE_ALL);
        }
    }

    fn has_neighbor_rail(&self, l: &mut Level, pos: Pos) -> bool {
        match get_rail(l, pos) {
            None => false,
            Some(mut rail) => {
                rail.remove_soft_connections(l);
                rail.can_connect_to(self)
            }
        }
    }

    fn place(&mut self, l: &mut Level, powered: bool, always_place: bool, shape: RailShape) {
        let north = self.pos.relative(Dir::North);
        let south = self.pos.relative(Dir::South);
        let west = self.pos.relative(Dir::West);
        let east = self.pos.relative(Dir::East);
        let has_north = self.has_neighbor_rail(l, north);
        let has_south = self.has_neighbor_rail(l, south);
        let has_west = self.has_neighbor_rail(l, west);
        let has_east = self.has_neighbor_rail(l, east);
        let mut rail_shape = None;
        let flag = has_north || has_south;
        let flag1 = has_west || has_east;
        if flag && !flag1 {
            rail_shape = Some(NorthSouth);
        }
        if flag1 && !flag {
            rail_shape = Some(EastWest);
        }
        let flag2 = has_south && has_east;
        let flag3 = has_south && has_west;
        let flag4 = has_north && has_east;
        let flag5 = has_north && has_west;
        if !self.straight {
            if flag2 && !has_north && !has_west {
                rail_shape = Some(SouthEast);
            }
            if flag3 && !has_north && !has_east {
                rail_shape = Some(SouthWest);
            }
            if flag5 && !has_south && !has_east {
                rail_shape = Some(NorthWest);
            }
            if flag4 && !has_south && !has_west {
                rail_shape = Some(NorthEast);
            }
        }
        if rail_shape.is_none() {
            if flag && flag1 {
                rail_shape = Some(shape);
            } else if flag {
                rail_shape = Some(NorthSouth);
            } else if flag1 {
                rail_shape = Some(EastWest);
            }
            if !self.straight {
                if powered {
                    if flag2 {
                        rail_shape = Some(SouthEast);
                    }
                    if flag3 {
                        rail_shape = Some(SouthWest);
                    }
                    if flag4 {
                        rail_shape = Some(NorthEast);
                    }
                    if flag5 {
                        rail_shape = Some(NorthWest);
                    }
                } else {
                    if flag5 {
                        rail_shape = Some(NorthWest);
                    }
                    if flag4 {
                        rail_shape = Some(NorthEast);
                    }
                    if flag3 {
                        rail_shape = Some(SouthWest);
                    }
                    if flag2 {
                        rail_shape = Some(SouthEast);
                    }
                }
            }
        }
        if rail_shape == Some(NorthSouth) {
            if is_rail_at(l, north.above()) {
                rail_shape = Some(AscendingNorth);
            }
            if is_rail_at(l, south.above()) {
                rail_shape = Some(AscendingSouth);
            }
        }
        if rail_shape == Some(EastWest) {
            if is_rail_at(l, east.above()) {
                rail_shape = Some(AscendingEast);
            }
            if is_rail_at(l, west.above()) {
                rail_shape = Some(AscendingWest);
            }
        }
        let rail_shape = rail_shape.unwrap_or(shape);
        self.update_connections(rail_shape);
        self.state = with_shape(l, self.state, rail_shape);
        if always_place || l.get(self.pos) != self.state {
            l.set(self.pos, self.state, UPDATE_ALL);
            if !self.is_valid(l) {
                return;
            }
            for i in 0..self.connections.len() {
                if let Some(mut rail) = get_rail(l, self.connections[i]) {
                    if rail.is_valid(l) {
                        rail.remove_soft_connections(l);
                        if rail.can_connect_to(self) {
                            rail.connect_to(l, self);
                        }
                    }
                }
            }
        }
    }
}

// RailState.hasRail.
fn has_rail(l: &mut Level, pos: Pos) -> bool {
    is_rail_at(l, pos) || is_rail_at(l, pos.above()) || is_rail_at(l, pos.below())
}

// RailState.getRail: the rail at, above or below the position.
fn get_rail(l: &mut Level, pos: Pos) -> Option<RailState> {
    let state = l.get(pos);
    if is_rail(l, state) {
        return Some(RailState::new(l, pos, state));
    }
    let above = pos.above();
    let state = l.get(above);
    if is_rail(l, state) {
        return Some(RailState::new(l, above, state));
    }
    let below = pos.below();
    let state = l.get(below);
    if is_rail(l, state) {
        Some(RailState::new(l, below, state))
    } else {
        None
    }
}

// --- BaseRailBlock ---

// BaseRailBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    can_support_rigid_block(l, pos.below())
}

// BaseRailBlock.onPlace, and DetectorRailBlock's.
fn on_place<const KIND: u8>(l: &mut Level, state: State, pos: Pos, old: State, moved: bool) {
    if l.block(old) != l.block(state) {
        let this = l.block(state);
        let state = update_state_moved::<KIND>(l, state, pos, moved);
        if KIND == DETECTOR_KIND {
            check_pressed(l, this, pos, state);
        }
    }
}

// BaseRailBlock.updateState(state, level, pos, movedByPiston).
fn update_state_moved<const KIND: u8>(l: &mut Level, mut state: State, pos: Pos, moved: bool) -> State {
    state = update_dir(l, pos, state, true);
    if KIND != RAIL_KIND {
        l.neighbor_changed_with(state, pos, l.block(state), moved);
        state = l.get(pos);
    }
    state
}

// BaseRailBlock.neighborChanged.
fn neighbor_changed<const KIND: u8>(l: &mut Level, state: State, pos: Pos, neighbor: BlockId, moved: bool) {
    let current = l.get(pos);
    if l.block(current) == l.block(state) {
        let rail_shape = shape(l, state);
        if should_be_removed(l, pos, rail_shape) {
            crate::entity::item::drop_resources(l, state, pos);
            l.remove_block(pos, moved);
        } else {
            update_state_neighbor::<KIND>(l, state, pos, neighbor);
        }
    }
}

// BaseRailBlock.shouldBeRemoved.
fn should_be_removed(l: &mut Level, pos: Pos, shape: RailShape) -> bool {
    if !can_support_rigid_block(l, pos.below()) {
        return true;
    }
    match shape {
        AscendingEast => !can_support_rigid_block(l, pos.relative(Dir::East)),
        AscendingWest => !can_support_rigid_block(l, pos.relative(Dir::West)),
        AscendingNorth => !can_support_rigid_block(l, pos.relative(Dir::North)),
        AscendingSouth => !can_support_rigid_block(l, pos.relative(Dir::South)),
        _ => false,
    }
}

// BaseRailBlock.updateDir.
fn update_dir(l: &mut Level, pos: Pos, state: State, always_place: bool) -> State {
    let rail_shape = shape(l, state);
    let mut rail = RailState::new(l, pos, state);
    let powered = l.has_neighbor_signal(pos);
    rail.place(l, powered, always_place, rail_shape);
    l.get(pos)
}

// updateState(state, level, pos, neighborBlock): RailBlock's and
// PoweredRailBlock's; the detector rail's is BaseRailBlock's, which does nothing.
fn update_state_neighbor<const KIND: u8>(l: &mut Level, state: State, pos: Pos, neighbor: BlockId) {
    match KIND {
        RAIL_KIND => {
            // RailBlock.updateState.
            let neighbor_default = l.reg.default_state(neighbor);
            if l.is_signal_source(neighbor_default) && RailState::new(l, pos, state).count_potential_connections(l) == 3 {
                update_dir(l, pos, state, false);
            }
        }
        POWERED_KIND => powered_update_state(l, state, pos),
        _ => {}
    }
}

// BaseRailBlock.affectNeighborsAfterRemoval.
fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, moved: bool) {
    if !moved {
        let block = l.block(state);
        if shape(l, state).is_slope() {
            l.update_neighbors_at(pos.above(), block);
        }
        if is_straight(l, state) {
            l.update_neighbors_at(pos, block);
            l.update_neighbors_at(pos.below(), block);
        }
    }
}

// BaseRailBlock.updateShape: a waterlogged rail schedules a water tick,
// then BlockBehaviour's returns the state.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    crate::blocks::shapes::common::schedule_water_tick(l, state, pos);
    state
}

// --- PoweredRailBlock ---

// PoweredRailBlock.findPoweredRailSignal.
fn find_powered_rail_signal(l: &mut Level, block: BlockId, pos: Pos, state: State, search_forward: bool, recursion_count: i32) -> bool {
    if recursion_count >= 8 {
        return false;
    }
    let (mut x, mut y, mut z) = (pos.x(), pos.y(), pos.z());
    let mut flag = true;
    let mut rail_shape = shape(l, state);
    match rail_shape {
        NorthSouth => {
            if search_forward {
                z += 1;
            } else {
                z -= 1;
            }
        }
        EastWest => {
            if search_forward {
                x -= 1;
            } else {
                x += 1;
            }
        }
        AscendingEast => {
            if search_forward {
                x -= 1;
            } else {
                x += 1;
                y += 1;
                flag = false;
            }
            rail_shape = EastWest;
        }
        AscendingWest => {
            if search_forward {
                x -= 1;
                y += 1;
                flag = false;
            } else {
                x += 1;
            }
            rail_shape = EastWest;
        }
        AscendingNorth => {
            if search_forward {
                z += 1;
            } else {
                z -= 1;
                y += 1;
                flag = false;
            }
            rail_shape = NorthSouth;
        }
        AscendingSouth => {
            if search_forward {
                z += 1;
                y += 1;
                flag = false;
            } else {
                z -= 1;
            }
            rail_shape = NorthSouth;
        }
        _ => {}
    }
    is_same_rail_with_power(l, block, Pos::new(x, y, z), search_forward, recursion_count, rail_shape)
        || flag && is_same_rail_with_power(l, block, Pos::new(x, y - 1, z), search_forward, recursion_count, rail_shape)
}

// PoweredRailBlock.isSameRailWithPower.
fn is_same_rail_with_power(l: &mut Level, block: BlockId, pos: Pos, search_forward: bool, recursion_count: i32, shape_: RailShape) -> bool {
    let state = l.get(pos);
    if l.block(state) != block {
        return false;
    }
    let rail_shape = shape(l, state);
    (shape_ != EastWest || rail_shape != NorthSouth && rail_shape != AscendingNorth && rail_shape != AscendingSouth)
        && (shape_ != NorthSouth || rail_shape != EastWest && rail_shape != AscendingEast && rail_shape != AscendingWest)
        && powered(l, state)
        && (l.has_neighbor_signal(pos) || find_powered_rail_signal(l, block, pos, state, search_forward, recursion_count + 1))
}

// PoweredRailBlock.updateState(state, level, pos, block).
fn powered_update_state(l: &mut Level, state: State, pos: Pos) {
    let block = l.block(state);
    let powered_value = powered(l, state);
    let flag = l.has_neighbor_signal(pos) || find_powered_rail_signal(l, block, pos, state, true, 0) || find_powered_rail_signal(l, block, pos, state, false, 0);
    if flag != powered_value {
        // CraftEventFactory.callRedstoneChange(level, pos, power, 15 - power):
        // with no listener the new current stays 15 - power, never power.
        let power = if flag { 15 } else { 0 };
        let new_power = 15 - power;
        if new_power == power {
            return;
        }
        l.set(pos, l.reg.with_bool(state, powered_property(l), flag), UPDATE_ALL);
        l.update_neighbors_at(pos.below(), block);
        if shape(l, state).is_slope() {
            l.update_neighbors_at(pos.above(), block);
        }
    }
}

// --- DetectorRailBlock ---

// DetectorRailBlock.getSearchBB.
fn search_bb(pos: Pos) -> Aabb {
    Aabb::new(pos.x() as f64 + 0.2, pos.y() as f64, pos.z() as f64 + 0.2, (pos.x() + 1) as f64 - 0.2, (pos.y() + 1) as f64 - 0.2, (pos.z() + 1) as f64 - 0.2)
}

// DetectorRailBlock.entityInside: EntityInsideBlockEvent has no listener.
fn detector_entity_inside(l: &mut Level, state: State, pos: Pos, _entity: &mut crate::entity::Entity, _applier: &mut crate::entity::inside::Applier, _past_edges: bool) {
    if !powered(l, state) {
        check_pressed(l, l.block(state), pos, state);
    }
}

// DetectorRailBlock.tick.
fn detector_tick(l: &mut Level, state: State, pos: Pos) {
    if powered(l, state) {
        check_pressed(l, l.block(state), pos, state);
    }
}

fn detector_get_signal(l: &mut Level, state: State, _pos: Pos, _side: Dir) -> i32 {
    if powered(l, state) {
        15
    } else {
        0
    }
}

fn detector_get_direct_signal(l: &mut Level, state: State, _pos: Pos, side: Dir) -> i32 {
    if !powered(l, state) {
        0
    } else if side == Dir::Up {
        15
    } else {
        0
    }
}

// DetectorRailBlock.getAnalogOutputSignal: the command block minecart (none
// here) and then the first container minecart on the rail.
fn detector_get_analog_output_signal(l: &mut Level, state: State, pos: Pos, _direction: Dir) -> i32 {
    if powered(l, state) {
        let carts = crate::entity::minecarts_in(l, &search_bb(pos), crate::entity::Only::Containers);
        if let Some(&first) = carts.first() {
            return super::container::redstone_signal(l, &super::container::Cont::Entity(first));
        }
    }
    0
}

// DetectorRailBlock.checkPressed.
fn check_pressed(l: &mut Level, block: BlockId, pos: Pos, state: State) {
    if !can_survive(l, state, pos) {
        return;
    }
    if l.block(state) != block {
        return;
    }
    let powered_value = powered(l, state);
    // BlockRedstoneEvent has no listener: the new current stands.
    let flag = !crate::entity::minecarts_in(l, &search_bb(pos), crate::entity::Only::Carts).is_empty();
    if flag && !powered_value {
        let powered_state = l.reg.with_bool(state, powered_property(l), true);
        l.set(pos, powered_state, UPDATE_ALL);
        update_power_to_connected(l, pos, powered_state);
        l.update_neighbors_at(pos, block);
        l.update_neighbors_at(pos.below(), block);
    }
    if !flag && powered_value {
        let unpowered_state = l.reg.with_bool(state, powered_property(l), false);
        l.set(pos, unpowered_state, UPDATE_ALL);
        update_power_to_connected(l, pos, unpowered_state);
        l.update_neighbors_at(pos, block);
        l.update_neighbors_at(pos.below(), block);
    }
    if flag {
        l.schedule_tick(pos, block, 20, PRIORITY_NORMAL);
    }
    l.update_neighbour_for_output_signal(pos, block);
}

// DetectorRailBlock.updatePowerToConnected.
fn update_power_to_connected(l: &mut Level, pos: Pos, state: State) {
    let rail = RailState::new(l, pos, state);
    for block_pos in rail.connections {
        let block_state = l.get(block_pos);
        l.neighbor_changed_with(block_state, block_pos, l.block(block_state), false);
    }
}
