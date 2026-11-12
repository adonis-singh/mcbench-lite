// FarmBlock: farmland. updateShape schedules the tick that turns it to dirt
// when something solid is on top; fallOn draws from the level random for a
// landing entity; the moisture changes are random ticks, which never run.

use crate::blocks::moving_piston;
use crate::blocks::{Behaviour, NONE};
use crate::collision::{self, Moved};
use crate::dir::{Dir, Pos};
use crate::game_event::Context;
use crate::entity::{self, motion, Entity, Kind};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::phys::Aabb;
use crate::registry::{Shape, State, SOLID};
use crate::{block_id, shape_classes, shape_ops};

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    can_survive: Some(can_survive),
    tick: Some(tick),
    fall_on: Some(fall_on),
    all_entities: true,
    ..NONE
};

// FarmBlock.updateShape: a block above that the farmland cannot bear
// schedules its tick one tick on; the state stays.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction == Dir::Up && !can_survive(l, state, pos) {
        l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
    }
    state
}

// FarmBlock.canSurvive: nothing solid above, except a fence gate or a moving
// piston.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let above = l.get(pos.above());
    survives_under(l, above) || moving_piston::is_moving_piston(l, above)
}

// DirtPathBlock.canSurvive, and FarmBlock's before its moving piston: nothing
// solid above, except a fence gate.
pub fn survives_under(l: &Level, above: State) -> bool {
    l.reg.flags(above) & SOLID == 0 || shape_classes!(l, ["FenceGateBlock"])[l.block(above) as usize]
}

// FarmBlock.tick: farmland that cannot survive turns to dirt.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if !can_survive(l, state, pos) {
        turn_to_dirt(l, state, pos);
    }
}

// FarmBlock.fallOn: Block.fallOn (Entity.causeFallDamage), then the draw
// level.random.nextFloat() < fallDistance - 0.5, which is made for every entity
// that lands. Only a living entity (the armor stand) turns the farmland to dirt,
// and only a big one: width * width * height over 0.512.
fn fall_on(l: &mut Level, _state: State, _pos: Pos, entity: &mut Entity, fall_distance: f64) {
    motion::cause_fall_damage(l, entity, fall_distance);
    let draw = l.random.next_float();
    if (draw as f64) < fall_distance - 0.5 && matches!(entity.kind, Kind::ArmorStand(_)) {
        // (An armor stand is 0.5 wide and 1.975 high, or half that: 0.49375 at most.)
        assert!(!(entity.width * entity.width * entity.height > 0.512f32), "an armor stand big enough to trample farmland");
    }
}

// FarmBlock.turnToDirt(null, state, level, pos): the fade event has no
// listener; entities in the way are pushed up, then the block is set, and
// sculk sensors hear the block change.
pub fn turn_to_dirt(l: &mut Level, state: State, pos: Pos) {
    let dirt = l.reg.default_state(block_id!(l, "minecraft:dirt"));
    push_entities_up(l, state, dirt, pos);
    l.set(pos, dirt, UPDATE_ALL);
    l.game_event_in(crate::game_event!(l, "block_change"), pos, Context::of_state(dirt));
}

// Block.pushEntitiesUp(oldState, newState, level, pos) for a block that was
// a column of 15 sixteenths and becomes a full block (farmland and dirt
// path to dirt): Shapes.joinUnoptimized(old, new, ONLY_SECOND) is then the
// top voxel of the cube, in a shape whose y coordinates are 0, the old top
// and 1 (MergedVoxelCoordinateList.merge); every entity touching it is
// moved up onto the new top.
fn push_entities_up(l: &mut Level, old: State, new: State, pos: Pos) {
    let old_box = shape_ops::collision_shape_at(l, old, pos).single.expect("a column as the old collision shape");
    let new_box = shape_ops::collision_shape_at(l, new, pos).single.expect("a block as the new collision shape");
    assert!(
        old_box[0] == 0.0 && old_box[1] == 0.0 && old_box[2] == 0.0 && old_box[3] == 1.0 && old_box[5] == 1.0 && old_box[4] < 1.0 && new_box == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        "pushEntitiesUp from a column to a full block"
    );
    let shape = Shape::new([vec![0.0, 1.0], vec![0.0, old_box[4], 1.0], vec![0.0, 1.0]], [1, 2, 1], vec![false, true], None, false);
    let off = [pos.x() as f64, pos.y() as f64, pos.z() as f64];
    // VoxelShape.bounds() of the moved shape.
    let bounds = Aabb::new(shape.coords[0][0] + off[0], shape.coords[1][1] + off[1], shape.coords[2][0] + off[2], shape.coords[0][1] + off[0], shape.coords[1][2] + off[1], shape.coords[2][1] + off[2]);
    // level.getEntities(null, bounds) with EntitySelector.NO_SPECTATORS.
    for id in entity::entities_in(l, None, None, entity::Only::Any, &bounds, |_| true) {
        let mut e = l.entities.all.remove(&id).expect("an entity of the lookup");
        // Shapes.collide(Axis.Y, bb.move(0, 1, 0), List.of(shape), -1.0).
        let d = collision::collide_voxels(1, &Moved { shape: std::borrow::Cow::Borrowed(&shape), off }, &e.bb.mv(0.0, 1.0, 0.0), -1.0);
        motion::teleport_relative(l, &mut e, 0.0, 1.0 + d, 0.0);
        l.entities.all.insert(id, e);
    }
}
