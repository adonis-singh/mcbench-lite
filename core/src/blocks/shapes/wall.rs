// WallBlock. A wall's sides and post depend on the collision shape of the
// block above (VoxelShape.getFaceShape(DOWN)), which shape_ops reads from the
// registry's collision shapes.

use super::common::{schedule_water_tick, turns};
use super::iron_bars::is_iron_bars;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::{Shape, State, Support};
use crate::{shape_classes, shape_tag, shape_ops};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), ..NONE };

// WallSide's indices in its property's values.
const NONE_SIDE: u16 = 0;
const LOW: u16 = 1;
const TALL: u16 = 2;

fn side_property(l: &Level, dir: Dir) -> u16 {
    match dir {
        Dir::North => property!(l, "north", ["none", "low", "tall"]),
        Dir::East => property!(l, "east", ["none", "low", "tall"]),
        Dir::South => property!(l, "south", ["none", "low", "tall"]),
        Dir::West => property!(l, "west", ["none", "low", "tall"]),
        _ => panic!("{dir:?} has no wall side"),
    }
}

fn up(l: &Level) -> u16 {
    property!(l, "up", ["true", "false"])
}

const SIDES: [Dir; 4] = [Dir::North, Dir::East, Dir::South, Dir::West];

// WallBlock.isCovered(face, test).
fn is_covered(face: &Shape, test: &Shape) -> bool {
    !shape_ops::join_is_not_empty(test, face, shape_ops::only_first)
}

// WallBlock.TEST_SHAPE_POST: Block.column(2.0, 0.0, 16.0).
fn test_post() -> Shape {
    shape_ops::block_box(7.0, 0.0, 7.0, 9.0, 16.0, 9.0)
}

// WallBlock.TEST_SHAPES_WALL: Shapes.rotateHorizontal(Block.boxZ(2.0, 16.0,
// 0.0, 9.0)), which faces north and turns clockwise a quarter at a time.
fn test_wall(dir: Dir) -> Shape {
    let (mut x0, mut x1, mut z0, mut z1) = (7.0, 9.0, 0.0, 9.0);
    for _ in 0..turns(dir) {
        (x0, x1, z0, z1) = (16.0 - z1, 16.0 - z0, x0, x1);
    }
    shape_ops::block_box(x0, 0.0, z0, x1, 16.0, z1)
}

// getCollisionShape(level, pos).getFaceShape(Direction.DOWN) of a state.
fn down_face(l: &Level, state: State, pos: Pos) -> Shape {
    shape_ops::collision_shape_at(l, state, pos).face_shape(Dir::Down)
}

fn side_name(dir: Dir) -> &'static str {
    dir.name()
}

// WallBlock.connectsTo.
fn connects_to(l: &Level, state: State, side_solid: bool, direction: Dir) -> bool {
    let block = l.block(state) as usize;
    let gate = shape_classes!(l, ["FenceGateBlock"])[block] && super::common::gate_connects_to_direction(l, state, direction);
    shape_tag!(l, "minecraft:walls")[block] || !super::common::is_exception_for_connection(l, state) && side_solid || is_iron_bars(l, state) || gate
}

fn is_connected(l: &Level, state: State, dir: Dir) -> bool {
    l.reg.get(state, side_property(l, dir)) != NONE_SIDE
}

// WallBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    if direction == Dir::Down {
        state
    } else if direction == Dir::Up {
        // topUpdate.
        let connected = SIDES.map(|dir| is_connected(l, state, dir));
        update_state(l, state, pos.above(), neighbor, connected)
    } else {
        // sideUpdate.
        let opposite = direction.opposite();
        let mut connected = SIDES.map(|dir| is_connected(l, state, dir));
        let at = SIDES.iter().position(|&dir| dir == direction).unwrap();
        connected[at] = connects_to(l, neighbor, shape_ops::is_face_sturdy(l, neighbor, neighbor_pos, opposite, Support::Full), opposite);
        let above = l.get(pos.above());
        update_state(l, state, pos.above(), above, connected)
    }
}

// WallBlock.updateShape(level, state, pos, neighborState, north, east, south,
// west): the block above is the neighbour.
fn update_state(l: &mut Level, state: State, above_pos: Pos, above: State, connected: [bool; 4]) -> State {
    let face = down_face(l, above, above_pos);
    let mut state = state;
    // updateSides.
    for (i, &dir) in SIDES.iter().enumerate() {
        let side = if connected[i] {
            if is_covered(&face, &test_wall(dir)) {
                TALL
            } else {
                LOW
            }
        } else {
            NONE_SIDE
        };
        state = l.reg.with(state, side_property(l, dir), side);
    }
    let raise = should_raise_post(l, state, above, &face);
    l.reg.with_bool(state, up(l), raise)
}

// WallBlock.shouldRaisePost.
fn should_raise_post(l: &Level, state: State, above: State, face: &Shape) -> bool {
    if shape_classes!(l, ["WallBlock"])[l.block(above) as usize] && l.reg.get_bool(above, up(l)) {
        return true;
    }
    let side = |dir: Dir| l.reg.get(state, side_property(l, dir));
    let (north, south, east, west) = (side(Dir::North), side(Dir::South), side(Dir::East), side(Dir::West));
    let (no_south, no_west, no_east, no_north) = (south == NONE_SIDE, west == NONE_SIDE, east == NONE_SIDE, north == NONE_SIDE);
    if no_north && no_south && no_west && no_east || no_north != no_south || no_west != no_east {
        return true;
    }
    let tall_line = north == TALL && south == TALL || east == TALL && west == TALL;
    !tall_line && (shape_tag!(l, "minecraft:wall_post_override")[l.block(above) as usize] || is_covered(face, &test_post()))
}
