// RedStoneWireBlock, on the path Paper's defaults take: redstone-implementation
// VANILLA (not EIGENCRAFT or ALTERNATE_CURRENT) and the redstone experiments
// feature off, so the wire's evaluator is DefaultRedstoneWireEvaluator.
// Events (BlockRedstoneEvent) have no listener: the new current stands.

use super::{Behaviour, InputPlayer, NONE};
use crate::dir::{Dir, Pos, ALL, HORIZONTAL};
use crate::level::{refuse, Level, UPDATE_ALL, UPDATE_CLIENTS};
use crate::property;
use crate::registry::{BlockId, State, Support};
use std::sync::OnceLock;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(neighbor_changed),
    on_place: Some(on_place),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    update_indirect_neighbour_shapes: Some(update_indirect_neighbour_shapes),
    get_signal: Some(get_signal),
    get_direct_signal: Some(get_direct_signal),
    can_survive: Some(can_survive),
    use_without_item: Some(use_without_item),
    ..NONE
};

// RedstoneSide's indices in its property's values.
const UP: u16 = 0;
const SIDE: u16 = 1;
const NO_SIDE: u16 = 2;

// RedStoneWireBlock.PROPERTY_BY_DIRECTION.
fn side_property(l: &Level, dir: Dir) -> u16 {
    match dir {
        Dir::North => property!(l, "north", ["up", "side", "none"]),
        Dir::East => property!(l, "east", ["up", "side", "none"]),
        Dir::South => property!(l, "south", ["up", "side", "none"]),
        Dir::West => property!(l, "west", ["up", "side", "none"]),
        _ => panic!("{dir:?} has no wire side"),
    }
}

// RedStoneWireBlock.POWER.
fn power_property(l: &Level) -> u16 {
    property!(l, "power", ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15"])
}

fn side(l: &Level, state: State, dir: Dir) -> u16 {
    l.reg.get(state, side_property(l, dir))
}

fn with_side(l: &Level, state: State, dir: Dir, value: u16) -> State {
    l.reg.with(state, side_property(l, dir), value)
}

fn power(l: &Level, state: State) -> i32 {
    l.reg.get(state, power_property(l)) as i32
}

fn with_power(l: &Level, state: State, value: i32) -> State {
    l.reg.with(state, power_property(l), value as u16)
}

// RedstoneSide.isConnected.
fn connected(side: u16) -> bool {
    side != NO_SIDE
}

fn is_wire(l: &Level, state: State) -> bool {
    l.block(state) == l.known.redstone_wire
}

fn default_state(l: &Level) -> State {
    l.reg.default_state(l.known.redstone_wire)
}

// RedStoneWireBlock.crossState.
fn cross_state(l: &Level) -> State {
    HORIZONTAL.iter().fold(default_state(l), |state, &dir| with_side(l, state, dir, SIDE))
}

// RedStoneWireBlock.isCross.
fn is_cross(l: &Level, state: State) -> bool {
    HORIZONTAL.iter().all(|&dir| connected(side(l, state, dir)))
}

// RedStoneWireBlock.isDot.
fn is_dot(l: &Level, state: State) -> bool {
    HORIZONTAL.iter().all(|&dir| !connected(side(l, state, dir)))
}

// instanceof TrapDoorBlock, by block.
fn is_trapdoor(l: &Level, state: State) -> bool {
    static TRAPDOORS: OnceLock<Vec<bool>> = OnceLock::new();
    TRAPDOORS.get_or_init(|| l.reg.blocks.iter().map(|block| matches!(block.class.as_str(), "TrapDoorBlock" | "WeatheringCopperTrapDoorBlock")).collect())[l.block(state) as usize]
}

fn observer(l: &Level) -> BlockId {
    static OBSERVER: OnceLock<BlockId> = OnceLock::new();
    *OBSERVER.get_or_init(|| l.reg.block_key("minecraft:observer"))
}

// RedStoneWireBlock.getConnectionState.
fn get_connection_state(l: &mut Level, state: State, pos: Pos) -> State {
    let was_dot = is_dot(l, state);
    let blank = with_power(l, default_state(l), power(l, state));
    let mut state = get_missing_connections(l, blank, pos);
    if was_dot && is_dot(l, state) {
        return state;
    }
    let north = connected(side(l, state, Dir::North));
    let south = connected(side(l, state, Dir::South));
    let east = connected(side(l, state, Dir::East));
    let west = connected(side(l, state, Dir::West));
    let no_north_south = !north && !south;
    let no_east_west = !east && !west;
    if !west && no_north_south {
        state = with_side(l, state, Dir::West, SIDE);
    }
    if !east && no_north_south {
        state = with_side(l, state, Dir::East, SIDE);
    }
    if !north && no_east_west {
        state = with_side(l, state, Dir::North, SIDE);
    }
    if !south && no_east_west {
        state = with_side(l, state, Dir::South, SIDE);
    }
    state
}

// RedStoneWireBlock.useWithoutItem, by a player who may build (the input
// player is a survival player): a cross becomes a dot and a dot a cross, as
// far as the wire's connections let it; dust that is neither, or whose
// connections make the same state either way, passes.
fn use_without_item(l: &mut Level, state: State, pos: Pos, _player: Option<InputPlayer>) -> bool {
    if !is_cross(l, state) && !is_dot(l, state) {
        return false;
    }
    let toggled = if is_cross(l, state) { default_state(l) } else { cross_state(l) };
    let toggled = with_power(l, toggled, power(l, state));
    let toggled = get_connection_state(l, toggled, pos);
    if toggled == state {
        return false;
    }
    l.set(pos, toggled, UPDATE_ALL);
    updates_on_shape_change(l, pos, state, toggled);
    true
}

// RedStoneWireBlock.updatesOnShapeChange: a conductor on a side whose
// connection changed updates its neighbours but the wire (the redstone
// experiments are off: no orientation).
fn updates_on_shape_change(l: &mut Level, pos: Pos, old: State, new: State) {
    for dir in HORIZONTAL {
        let at = pos.relative(dir);
        if connected(side(l, old, dir)) != connected(side(l, new, dir)) {
            let neighbor = l.get(at);
            if l.is_conductor(neighbor) {
                l.update_neighbors_at_except_from_facing(at, l.block(new), dir.opposite());
            }
        }
    }
}

// RedStoneWireBlock.getMissingConnections.
fn get_missing_connections(l: &mut Level, mut state: State, pos: Pos) -> State {
    let above = l.get(pos.above());
    let non_normal_cube_above = !l.is_conductor(above);
    for dir in HORIZONTAL {
        if !connected(side(l, state, dir)) {
            let connecting = get_connecting_side(l, pos, dir, non_normal_cube_above);
            state = with_side(l, state, dir, connecting);
        }
    }
    state
}

// RedStoneWireBlock.getConnectingSide(level, pos, face).
fn get_connecting_side_of(l: &mut Level, pos: Pos, face: Dir) -> u16 {
    let above = l.get(pos.above());
    get_connecting_side(l, pos, face, !l.is_conductor(above))
}

// RedStoneWireBlock.getConnectingSide(level, pos, direction, nonNormalCubeAbove).
fn get_connecting_side(l: &mut Level, pos: Pos, dir: Dir, non_normal_cube_above: bool) -> u16 {
    let at = pos.relative(dir);
    let state = l.get(at);
    if non_normal_cube_above {
        let flag = is_trapdoor(l, state) || can_survive_on(l, state, at);
        if flag {
            let above = l.get(at.above());
            if should_connect_to(l, above, None) {
                if crate::shape_ops::is_face_sturdy(l, state, at, dir.opposite(), Support::Full) {
                    return UP;
                }
                return SIDE;
            }
        }
    }
    if should_connect_to(l, state, Some(dir)) {
        return SIDE;
    }
    if !l.is_conductor(state) {
        let below = l.get(at.below());
        if should_connect_to(l, below, None) {
            return SIDE;
        }
    }
    NO_SIDE
}

// RedStoneWireBlock.shouldConnectTo(state, direction).
fn should_connect_to(l: &Level, state: State, direction: Option<Dir>) -> bool {
    let block = l.block(state);
    if block == l.known.redstone_wire {
        true
    } else if block == l.known.repeater {
        let facing = l.reg.get_dir(state, l.reg.property_of(state, "facing"));
        direction.is_some_and(|d| facing == d || facing.opposite() == d)
    } else if block == observer(l) {
        direction == Some(l.reg.get_dir(state, l.reg.property_of(state, "facing")))
    } else {
        l.is_signal_source(state) && direction.is_some()
    }
}

// RedStoneWireBlock.canSurviveOn.
fn can_survive_on(l: &Level, state: State, pos: Pos) -> bool {
    crate::shape_ops::is_face_sturdy(l, state, pos, Dir::Up, Support::Full) || l.block(state) == l.known.hopper
}

// RedStoneWireBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    can_survive_on(l, below, pos.below())
}

// RedStoneWireBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    if direction == Dir::Down {
        if can_survive_on(l, neighbor, neighbor_pos) {
            state
        } else {
            l.known.air
        }
    } else if direction == Dir::Up {
        get_connection_state(l, state, pos)
    } else {
        let connecting = get_connecting_side_of(l, pos, direction);
        if connected(connecting) == connected(side(l, state, direction)) && !is_cross(l, state) {
            with_side(l, state, direction, connecting)
        } else {
            let crossed = with_side(l, with_power(l, cross_state(l), power(l, state)), direction, connecting);
            get_connection_state(l, crossed, pos)
        }
    }
}

// RedStoneWireBlock.updateIndirectNeighbourShapes.
fn update_indirect_neighbour_shapes(l: &mut Level, state: State, pos: Pos, flags: i32, recursion: i32) {
    for dir in HORIZONTAL {
        if side(l, state, dir) != NO_SIDE {
            let beside = l.get(pos.relative(dir));
            if !is_wire(l, beside) {
                let below = pos.relative(dir).below();
                let below_state = l.get(below);
                if is_wire(l, below_state) {
                    let from = below.relative(dir.opposite());
                    let from_state = l.get(from);
                    l.neighbor_shape_changed(dir.opposite(), below, from, from_state, flags, recursion);
                }
                let above = pos.relative(dir).above();
                let above_state = l.get(above);
                if is_wire(l, above_state) {
                    let from = above.relative(dir.opposite());
                    let from_state = l.get(from);
                    l.neighbor_shape_changed(dir.opposite(), above, from, from_state, flags, recursion);
                }
            }
        }
    }
}

// RedStoneWireBlock.onPlace: updateSurroundingRedstone(level, pos, state,
// null, true) is updatePowerStrength on the vanilla path.
fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    if l.block(old) != l.block(state) {
        update_power_strength(l, pos, state);
        for dir in [Dir::Up, Dir::Down] {
            l.update_neighbors_at(pos.relative(dir), l.known.redstone_wire);
        }
        update_neighbors_of_neighboring_wires(l, pos);
    }
}

// RedStoneWireBlock.affectNeighborsAfterRemoval.
fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, moved: bool) {
    if !moved {
        for dir in ALL {
            l.update_neighbors_at(pos.relative(dir), l.known.redstone_wire);
        }
        update_power_strength(l, pos, state);
        update_neighbors_of_neighboring_wires(l, pos);
    }
}

// RedStoneWireBlock.updateNeighborsOfNeighboringWires.
fn update_neighbors_of_neighboring_wires(l: &mut Level, pos: Pos) {
    for dir in HORIZONTAL {
        check_corner_change_at(l, pos.relative(dir));
    }
    for dir in HORIZONTAL {
        let at = pos.relative(dir);
        let state = l.get(at);
        if l.is_conductor(state) {
            check_corner_change_at(l, at.above());
        } else {
            check_corner_change_at(l, at.below());
        }
    }
}

// RedStoneWireBlock.checkCornerChangeAt.
fn check_corner_change_at(l: &mut Level, pos: Pos) {
    let state = l.get(pos);
    if is_wire(l, state) {
        l.update_neighbors_at(pos, l.known.redstone_wire);
        for dir in ALL {
            l.update_neighbors_at(pos.relative(dir), l.known.redstone_wire);
        }
    }
}

// RedStoneWireBlock.neighborChanged. A wire that can no longer survive drops
// its item before it is removed.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    if can_survive(l, state, pos) {
        update_power_strength(l, pos, state);
    } else {
        crate::entity::item::drop_resources(l, state, pos);
        l.remove_block(pos, false);
    }
}

// RedStoneWireBlock.getDirectSignal.
fn get_direct_signal(l: &mut Level, state: State, pos: Pos, side: Dir) -> i32 {
    if !l.wire_should_signal {
        0
    } else {
        get_signal(l, state, pos, side)
    }
}

// RedStoneWireBlock.getSignal.
fn get_signal(l: &mut Level, state: State, pos: Pos, side_dir: Dir) -> i32 {
    if l.wire_should_signal && side_dir != Dir::Down {
        let strength = power(l, state);
        if strength == 0 {
            0
        } else if side_dir != Dir::Up && {
            let connections = get_connection_state(l, state, pos);
            !connected(side(l, connections, side_dir.opposite()))
        } {
            0
        } else {
            strength
        }
    } else {
        0
    }
}

// RedStoneWireBlock.getBlockSignal.
fn get_block_signal(l: &mut Level, pos: Pos) -> i32 {
    l.wire_should_signal = false;
    let best = l.best_neighbor_signal(pos);
    l.wire_should_signal = true;
    best
}

// RedstoneWireEvaluator.getWireSignal.
fn get_wire_signal(l: &Level, state: State) -> i32 {
    if is_wire(l, state) {
        power(l, state)
    } else {
        0
    }
}

// RedstoneWireEvaluator.getIncomingWireSignal.
fn get_incoming_wire_signal(l: &mut Level, pos: Pos) -> i32 {
    let mut i = 0;
    for dir in HORIZONTAL {
        let at = pos.relative(dir);
        let state = l.get(at);
        i = i.max(get_wire_signal(l, state));
        let above = pos.above();
        // The block above is read only when the one beside is a conductor.
        let beside_conducts = l.is_conductor(state);
        let open_above = beside_conducts && {
            let above_state = l.get(above);
            !l.is_conductor(above_state)
        };
        if open_above {
            let up = at.above();
            let up_state = l.get(up);
            i = i.max(get_wire_signal(l, up_state));
        } else if !l.is_conductor(state) {
            let down = at.below();
            let down_state = l.get(down);
            i = i.max(get_wire_signal(l, down_state));
        }
    }
    0.max(i - 1)
}

// DefaultRedstoneWireEvaluator.calculateTargetStrength.
fn calculate_target_strength(l: &mut Level, pos: Pos) -> i32 {
    let block_signal = get_block_signal(l, pos);
    if block_signal == 15 {
        block_signal
    } else {
        block_signal.max(get_incoming_wire_signal(l, pos))
    }
}

// DefaultRedstoneWireEvaluator.updatePowerStrength.
fn update_power_strength(l: &mut Level, pos: Pos, state: State) {
    let target = calculate_target_strength(l, pos);
    let old_power = power(l, state);
    if old_power != target {
        if l.get(pos) == state {
            l.set(pos, with_power(l, state, target), UPDATE_CLIENTS);
        }
        let mut set = [pos; 7];
        for (i, dir) in ALL.iter().enumerate() {
            set[i + 1] = pos.relative(*dir);
        }
        for at in hash_set_order(set) {
            l.update_neighbors_at(at, l.known.redstone_wire);
        }
    }
}

// Iterating a HashSet<BlockPos> filled in this order: Vec3i.hashCode into
// HashMap's 16 buckets (seven entries never resize or treeify the table),
// bucket by bucket, insertion order within a bucket.
fn hash_set_order(mut set: [Pos; 7]) -> [Pos; 7] {
    let bucket = |p: Pos| {
        let h = p.y().wrapping_add(p.z().wrapping_mul(31)).wrapping_mul(31).wrapping_add(p.x());
        (h ^ ((h as u32) >> 16) as i32) & 15
    };
    set.sort_by_key(|&p| bucket(p));
    set
}
