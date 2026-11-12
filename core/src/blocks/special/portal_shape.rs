// PortalShape: the obsidian frame a nether portal fills, as fire and the
// portal block itself look for it.

use crate::dir::{Axis, Dir, Pos};
use crate::level::Level;
use crate::registry::State;
use crate::world::MIN_Y;

#[derive(Clone, Copy)]
pub struct PortalShape {
    axis: Axis,
    right_dir: Dir,
    num_portal_blocks: i32,
    bottom_left: Pos,
    height: i32,
    width: i32,
}

// BlockBehaviour.StatePredicate FRAME.
fn is_frame(l: &Level, state: State) -> bool {
    l.reg.blocks[l.block(state) as usize].key == "minecraft:obsidian"
}

// PortalShape.isEmpty.
fn is_empty(l: &Level, state: State) -> bool {
    l.is_air(state) || l.reg.in_tag(state, "minecraft:fire") || l.reg.blocks[l.block(state) as usize].key == "minecraft:nether_portal"
}

// PortalShape.findEmptyPortalShape / findPortalShape: the shape in this axis
// or else the other, that is valid and has no portal blocks.
pub fn find_empty_portal_shape(l: &mut Level, bottom_left: Pos, axis: Axis) -> Option<PortalShape> {
    let first = find_any_shape(l, bottom_left, axis);
    if first.is_valid() && first.num_portal_blocks == 0 {
        return Some(first);
    }
    let other = if axis == Axis::X { Axis::Z } else { Axis::X };
    let second = find_any_shape(l, bottom_left, other);
    (second.is_valid() && second.num_portal_blocks == 0).then_some(second)
}

// PortalShape.findAnyShape.
pub fn find_any_shape(l: &mut Level, bottom_left: Pos, axis: Axis) -> PortalShape {
    let direction = if axis == Axis::X { Dir::West } else { Dir::South };
    let Some(corner) = calculate_bottom_left(l, direction, bottom_left) else {
        return PortalShape { axis, num_portal_blocks: 0, right_dir: direction, bottom_left, width: 0, height: 0 };
    };
    let width = calculate_width(l, corner, direction);
    if width == 0 {
        return PortalShape { axis, num_portal_blocks: 0, right_dir: direction, bottom_left: corner, width: 0, height: 0 };
    }
    let mut portal_blocks = 0;
    let height = calculate_height(l, corner, direction, width, &mut portal_blocks);
    PortalShape { axis, num_portal_blocks: portal_blocks, right_dir: direction, bottom_left: corner, width, height }
}

fn calculate_bottom_left(l: &mut Level, direction: Dir, mut pos: Pos) -> Option<Pos> {
    let max = MIN_Y.max(pos.y() - 21);
    while pos.y() > max && {
        let below = l.get(pos.below());
        is_empty(l, below)
    } {
        pos = pos.below();
    }
    let opposite = direction.opposite();
    let i = distance_until_edge_above_frame(l, pos, opposite) - 1;
    if i < 0 {
        None
    } else {
        Some(pos.relative_n(opposite, i))
    }
}

fn calculate_width(l: &mut Level, bottom_left: Pos, direction: Dir) -> i32 {
    let distance = distance_until_edge_above_frame(l, bottom_left, direction);
    if (2..=21).contains(&distance) {
        distance
    } else {
        0
    }
}

fn distance_until_edge_above_frame(l: &mut Level, pos: Pos, direction: Dir) -> i32 {
    for i in 0..=21 {
        let at = pos.relative_n(direction, i);
        let state = l.get(at);
        if !is_empty(l, state) {
            if is_frame(l, state) {
                return i;
            }
            break;
        }
        let below = l.get(at.below());
        if !is_frame(l, below) {
            break;
        }
    }
    0
}

fn calculate_height(l: &mut Level, pos: Pos, direction: Dir, width: i32, portal_blocks: &mut i32) -> i32 {
    let until_top = distance_until_top(l, pos, direction, width, portal_blocks);
    if (3..=21).contains(&until_top) && has_top_frame(l, pos, direction, width, until_top) {
        until_top
    } else {
        0
    }
}

fn has_top_frame(l: &mut Level, pos: Pos, direction: Dir, width: i32, until_top: i32) -> bool {
    for i in 0..width {
        let at = pos.relative_n(Dir::Up, until_top).relative_n(direction, i);
        let state = l.get(at);
        if !is_frame(l, state) {
            return false;
        }
    }
    true
}

fn distance_until_top(l: &mut Level, pos: Pos, direction: Dir, width: i32, portal_blocks: &mut i32) -> i32 {
    for i in 0..21 {
        let left = l.get(pos.relative_n(Dir::Up, i).relative_n(direction, -1));
        if !is_frame(l, left) {
            return i;
        }
        let right = l.get(pos.relative_n(Dir::Up, i).relative_n(direction, width));
        if !is_frame(l, right) {
            return i;
        }
        for i1 in 0..width {
            let state = l.get(pos.relative_n(Dir::Up, i).relative_n(direction, i1));
            if !is_empty(l, state) {
                return i;
            }
            if l.reg.blocks[l.block(state) as usize].key == "minecraft:nether_portal" {
                *portal_blocks += 1;
            }
        }
    }
    21
}

impl PortalShape {
    // PortalShape.isValid.
    pub fn is_valid(&self) -> bool {
        (2..=21).contains(&self.width) && (3..=21).contains(&self.height)
    }

    // PortalShape.isComplete.
    pub fn is_complete(&self) -> bool {
        self.is_valid() && self.num_portal_blocks == self.width * self.height
    }

    // PortalShape.createPortalBlocks: PortalCreateEvent has no listener, and
    // the blocks are set with flags 18 in BlockPos.betweenClosed's order.
    pub fn create_portal_blocks(&self, l: &mut Level) -> bool {
        let portal = l.reg.block_key("minecraft:nether_portal");
        let axis = l.reg.property("axis", &["x", "z"]);
        let state = l.reg.with_name(l.reg.default_state(portal), axis, if self.axis == Axis::X { "x" } else { "z" });
        let far = self.bottom_left.relative_n(Dir::Up, self.height - 1).relative_n(self.right_dir, self.width - 1);
        let (min, max) = (Pos::new(self.bottom_left.x().min(far.x()), self.bottom_left.y().min(far.y()), self.bottom_left.z().min(far.z())), Pos::new(self.bottom_left.x().max(far.x()), self.bottom_left.y().max(far.y()), self.bottom_left.z().max(far.z())));
        for z in min.z()..=max.z() {
            for y in min.y()..=max.y() {
                for x in min.x()..=max.x() {
                    l.set(Pos::new(x, y, z), state, 18);
                }
            }
        }
        true
    }
}
