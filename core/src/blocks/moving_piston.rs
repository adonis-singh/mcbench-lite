// MovingPistonBlock and PistonMovingBlockEntity: the block a piston leaves
// where a block is moving, and the entity that moves it, ticking from
// progress 0 to 1 in half steps and then setting the moved block down. Each
// tick it pushes the entities in the moved block's way (moveCollidedEntities:
// slime launches them), carries those standing on honey (moveStuckEntities),
// and its collision shape (getCollisionShape, which every reader of the
// block's collision and support shape takes) follows its progress.

use super::piston;
use super::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos};
use crate::entity::{self, motion, EntityId, Only};
use crate::level::Level;
use crate::phys::{jmax, jmin, Aabb, Vec3};
use crate::property;
use crate::registry::{Registry, Shape, State};
use crate::shape_ops;
use std::cell::Cell;

thread_local! {
    // PistonMovingBlockEntity.NOCLIP: the direction of the piston whose push
    // is moving an entity, in which a moving block is not in the entity's way.
    static NOCLIP: Cell<Option<Dir>> = const { Cell::new(None) };
}

pub const BEHAVIOUR: Behaviour = Behaviour { trigger_event: Some(trigger_event), ticker: Some(ticker), ..NONE };

// PistonMovingBlockEntity: the block it moves, the direction of its piston,
// whether the piston extends, whether this is the piston's own part (its
// head going out or its base going in), its progress now and at the last
// tick, and the game time it last ticked.
#[derive(Clone, Debug)]
pub struct Moving {
    moved_state: State,
    direction: Dir,
    extending: bool,
    is_source_piston: bool,
    progress: f32,
    progress_o: f32,
    last_ticked: i64,
}

impl Moving {
    // MovingPistonBlock.newMovingBlockEntity.
    pub fn new(moved_state: State, direction: Dir, extending: bool, is_source_piston: bool) -> Moving {
        Moving { moved_state, direction, extending, is_source_piston, progress: 0.0, progress_o: 0.0, last_ticked: 0 }
    }

    // A block entity at a given progress, for the probe that compares its
    // shapes with the server's (probe.rs).
    pub fn at_progress(mut self, progress: f32) -> Moving {
        self.progress = progress;
        self
    }

    pub fn is_extending(&self) -> bool {
        self.extending
    }

    pub fn direction(&self) -> Dir {
        self.direction
    }

    // PistonMovingBlockEntity.isSourcePiston.
    pub fn is_source_piston(&self) -> bool {
        self.is_source_piston
    }

    // PistonMovingBlockEntity.getMovedState.
    pub fn moved_state(&self) -> State {
        self.moved_state
    }

    // getProgress(0.0F): Mth.lerp(0, progressO, progress).
    pub fn get_progress(&self) -> f32 {
        self.progress_o + 0.0 * (self.progress - self.progress_o)
    }

    pub fn last_ticked(&self) -> i64 {
        self.last_ticked
    }

    // PistonMovingBlockEntity.getMovementDirection (and getPushDirection).
    fn movement_direction(&self) -> Dir {
        if self.extending {
            self.direction
        } else {
            self.direction.opposite()
        }
    }

    // PistonMovingBlockEntity.getExtendedProgress.
    fn extended_progress(&self, progress: f32) -> f32 {
        if self.extending {
            progress - 1.0f32
        } else {
            1.0f32 - progress
        }
    }

    // Whether the moved block is a piston and this is its own part going in:
    // `!extending && isSourcePiston && movedState.getBlock() instanceof PistonBaseBlock`.
    fn is_retracting_base(&self, reg: &Registry) -> bool {
        !self.extending && self.is_source_piston && reg.blocks[reg.block(self.moved_state) as usize].class == "PistonBaseBlock"
    }
}

// The piston head with these properties.
fn head_state(l: &Level, facing: Dir, short: bool, sticky: bool) -> State {
    let reg = l.reg;
    let head = reg.default_state(piston::piston_head(l));
    let head = reg.with_dir(head, piston::facing(l), facing);
    let head = reg.with_bool(head, short_property(l), short);
    reg.with_name(head, piston::piston_type(l), if sticky { "sticky" } else { "normal" })
}

// PistonHeadBlock.SHORT.
fn short_property(l: &Level) -> u16 {
    property!(l, "short", ["true", "false"])
}

// PistonMovingBlockEntity.getCollisionRelatedBlockState.
fn collision_related_state(l: &Level, m: &Moving) -> State {
    if m.is_retracting_base(l.reg) {
        let sticky = l.block(m.moved_state) == piston::sticky_piston(l);
        head_state(l, l.reg.get_dir(m.moved_state, piston::facing(l)), m.progress > 0.25f32, sticky)
    } else {
        m.moved_state
    }
}

// BlockState.getCollisionShape(level, pos) of a state that is not a moving
// piston and is moved by one: the dumped shape (for no entity), moved by the
// block's offset where it has one.
fn state_shape(l: &Level, state: State, pos: Pos) -> std::borrow::Cow<'static, Shape> {
    assert!(l.block(state) != piston::moving_piston_block(l), "a moving piston moving a moving piston");
    shape_ops::collision_shape_at(l, state, pos)
}

// PistonMovingBlockEntity.getCollisionShape(level, pos).
pub fn collision_shape(l: &Level, m: &Moving, pos: Pos) -> Shape {
    let collision_shape = if m.is_retracting_base(l.reg) {
        let extended = l.reg.with_bool(m.moved_state, piston::extended(l), true);
        state_shape(l, extended, pos).into_owned()
    } else {
        shape_ops::empty()
    };
    let direction = NOCLIP.with(|c| c.get());
    if (m.progress as f64) < 1.0 && direction == Some(m.movement_direction()) {
        return collision_shape;
    }
    let block_state = if m.is_source_piston {
        // `this.extending != 1.0F - this.progress < 0.25F`.
        head_state(l, m.direction, m.extending != ((1.0f32 - m.progress) < 0.25f32), false)
    } else {
        m.moved_state
    };
    let extended_progress = m.extended_progress(m.progress);
    let (sx, sy, sz) = m.direction.offset();
    let d = (sx as f32 * extended_progress) as f64;
    let d1 = (sy as f32 * extended_progress) as f64;
    let d2 = (sz as f32 * extended_progress) as f64;
    shape_ops::or(&collision_shape, &state_shape(l, block_state, pos).moved(d, d1, d2))
}

// Runs `f` with PistonMovingBlockEntity.NOCLIP set (the probe's way of
// asking for the shape a push sees).
pub fn with_noclip<R>(direction: Option<Dir>, f: impl FnOnce() -> R) -> R {
    NOCLIP.with(|c| c.set(direction));
    let result = f();
    NOCLIP.with(|c| c.set(None));
    result
}

// MovingPistonBlock.getCollisionShape (and getBlockSupportShape, which is
// the collision shape): the block entity's, or none without one.
pub fn shape_at(l: &Level, pos: Pos) -> Shape {
    match l.block_entities.get(&pos) {
        Some(BlockEntity::Piston(m)) => collision_shape(l, m, pos),
        _ => shape_ops::empty(),
    }
}

pub fn is_moving_piston(l: &Level, state: State) -> bool {
    l.block(state) == piston::moving_piston_block(l)
}

// MovingPistonBlock.getTicker: PistonMovingBlockEntity.tick for its entity
// type.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(tick)
}

// BaseEntityBlock.triggerEvent: the entity's own is BlockEntity's, false;
// getBlockEntity finds none to make for this block.
fn trigger_event(l: &mut Level, _state: State, pos: Pos, _id: i32, _param: i32) -> bool {
    l.block_entity(pos);
    false
}

// PistonMovingBlockEntity.preRemoveSideEffects.
pub fn pre_remove_side_effects(l: &mut Level, moving: &Moving, pos: Pos) {
    final_tick(l, pos, moving);
}

// PistonMovingBlockEntity.finalTick: the block entity is removed, and the
// moving block becomes what it moved (or air, for the piston's own part).
pub fn final_tick(l: &mut Level, pos: Pos, moving: &Moving) {
    if moving.progress_o < 1.0 {
        l.remove_block_entity(pos);
        let state = l.get(pos);
        if is_moving_piston(l, state) {
            let state = if moving.is_source_piston { l.known.air } else { crate::engine::update_from_neighbour_shapes(l, moving.moved_state, pos) };
            l.set(pos, state, 3);
            l.neighbor_changed(pos, l.block(state));
        }
    }
}

// PistonMovingBlockEntity.tick.
fn tick(l: &mut Level, pos: Pos, _state: State) {
    let game_time = l.game_time;
    let Some(BlockEntity::Piston(moving)) = l.block_entities.get_mut(&pos) else { panic!("a moving piston ticks without its block entity") };
    moving.last_ticked = game_time;
    moving.progress_o = moving.progress;
    if moving.progress_o >= 1.0 {
        let moving = moving.clone();
        l.remove_block_entity(pos);
        let state = l.get(pos);
        if is_moving_piston(l, state) {
            let block_state = crate::engine::update_from_neighbour_shapes(l, moving.moved_state, pos);
            if l.is_air(block_state) {
                // 340 with allow-piston-duplication, 342 without.
                l.set(pos, moving.moved_state, 342);
                l.update_or_destroy(moving.moved_state, block_state, pos, 3, 512);
            } else {
                let waterlogged = property!(l, "waterlogged", ["true", "false"]);
                let block_state = if l.reg.has(block_state, waterlogged) && l.reg.get_bool(block_state, waterlogged) {
                    l.reg.with_bool(block_state, waterlogged, false)
                } else {
                    block_state
                };
                l.set(pos, block_state, 67);
                l.neighbor_changed(pos, l.block(block_state));
            }
        }
    } else {
        let moving = moving.clone();
        let f = moving.progress + 0.5f32;
        move_collided_entities(l, pos, f, &moving);
        move_stuck_entities(l, pos, f, &moving);
        let Some(BlockEntity::Piston(live)) = l.block_entities.get_mut(&pos) else { panic!("a moving piston lost its block entity while it moved entities") };
        live.progress = f;
        if live.progress >= 1.0 {
            live.progress = 1.0;
        }
    }
}

// PistonMovingBlockEntity.getMovement: how far a box is into the way of a
// head moving in a direction.
fn get_movement(head_shape: &Aabb, direction: Dir, facing: &Aabb) -> f64 {
    match direction {
        Dir::East => head_shape.max_x - facing.min_x,
        Dir::West => facing.max_x - head_shape.min_x,
        Dir::Up => head_shape.max_y - facing.min_y,
        Dir::Down => facing.max_y - head_shape.min_y,
        Dir::South => head_shape.max_z - facing.min_z,
        Dir::North => facing.max_z - head_shape.min_z,
    }
}

// PistonMovingBlockEntity.moveByPositionAndProgress.
fn move_by_position_and_progress(pos: Pos, aabb: &Aabb, m: &Moving) -> Aabb {
    let d = m.extended_progress(m.progress) as f64;
    let (sx, sy, sz) = m.direction.offset();
    aabb.mv(pos.x() as f64 + d * sx as f64, pos.y() as f64 + d * sy as f64, pos.z() as f64 + d * sz as f64)
}

// PistonMath.getMovementArea.
fn movement_area(bounds: &Aabb, dir: Dir, delta: f64) -> Aabb {
    let step = match dir {
        Dir::Up | Dir::South | Dir::East => 1.0,
        _ => -1.0,
    };
    let d = delta * step;
    let min = jmin(d, 0.0);
    let max = jmax(d, 0.0);
    match dir {
        Dir::West => Aabb::new(bounds.min_x + min, bounds.min_y, bounds.min_z, bounds.min_x + max, bounds.max_y, bounds.max_z),
        Dir::East => Aabb::new(bounds.max_x + min, bounds.min_y, bounds.min_z, bounds.max_x + max, bounds.max_y, bounds.max_z),
        Dir::Down => Aabb::new(bounds.min_x, bounds.min_y + min, bounds.min_z, bounds.max_x, bounds.min_y + max, bounds.max_z),
        Dir::Up => Aabb::new(bounds.min_x, bounds.max_y + min, bounds.min_z, bounds.max_x, bounds.max_y + max, bounds.max_z),
        Dir::North => Aabb::new(bounds.min_x, bounds.min_y, bounds.min_z + min, bounds.max_x, bounds.max_y, bounds.min_z + max),
        Dir::South => Aabb::new(bounds.min_x, bounds.min_y, bounds.max_z + min, bounds.max_x, bounds.max_y, bounds.max_z + max),
    }
}

// AABB.minmax.
fn minmax(a: &Aabb, b: &Aabb) -> Aabb {
    Aabb::new(jmin(a.min_x, b.min_x), jmin(a.min_y, b.min_y), jmin(a.min_z, b.min_z), jmax(a.max_x, b.max_x), jmax(a.max_y, b.max_y), jmax(a.max_z, b.max_z))
}

// AABB.intersect.
fn intersect(a: &Aabb, b: &Aabb) -> Aabb {
    Aabb::new(jmax(a.min_x, b.min_x), jmax(a.min_y, b.min_y), jmax(a.min_z, b.min_z), jmin(a.max_x, b.max_x), jmin(a.max_y, b.max_y), jmin(a.max_z, b.max_z))
}

// Resets NOCLIP when a push ends, even where a refusal unwinds.
struct NoclipReset;

impl Drop for NoclipReset {
    fn drop(&mut self) {
        NOCLIP.with(|c| c.set(None));
    }
}

// PistonMovingBlockEntity.moveEntityByPiston for the client player
// (LocalPlayer.move with MoverType.PISTON).
fn move_player_by_piston(l: &Level, p: &mut crate::player::Player, noclip_direction: Dir, progress: f64, direction: Dir) {
    NOCLIP.with(|c| c.set(Some(noclip_direction)));
    let _reset = NoclipReset;
    let old_position = p.pos;
    let (sx, sy, sz) = direction.offset();
    p.move_by_piston(l, Vec3::new(progress * sx as f64, progress * sy as f64, progress * sz as f64));
    p.apply_effects_between(l, old_position, p.pos);
    p.remove_latest_movement_recording();
}

// PistonMovingBlockEntity.moveEntityByPiston: the entity is moved with
// MoverType.PISTON, in the push's direction no moving block is in its way.
fn move_entity_by_piston(l: &mut Level, noclip_direction: Dir, e: &mut entity::Entity, progress: f64, direction: Dir) {
    // (NOCLIP is reset even where a refusal unwinds.)
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            NOCLIP.with(|c| c.set(None));
        }
    }
    NOCLIP.with(|c| c.set(Some(noclip_direction)));
    let _reset = Reset;
    let old_position = e.pos;
    let (sx, sy, sz) = direction.offset();
    motion::move_entity_as(l, e, motion::Mover::Piston, Vec3::new(progress * sx as f64, progress * sy as f64, progress * sz as f64));
    motion::apply_effects_between(l, e, old_position, e.pos);
    // removeLatestMovementRecording.
    e.movement_this_tick.pop_back();
}

// PistonMovingBlockEntity.fixEntityWithinPistonBase.
fn fix_entity_within_piston_base(l: &mut Level, pos: Pos, e: &mut entity::Entity, dir: Dir, progress: f64) {
    let bounding_box = e.bb;
    let aabb = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0).mv(pos.x() as f64, pos.y() as f64, pos.z() as f64);
    if bounding_box.intersects(&aabb) {
        let opposite = dir.opposite();
        let mut d = get_movement(&aabb, opposite, &bounding_box) + 0.01;
        let d1 = get_movement(&aabb, opposite, &intersect(&bounding_box, &aabb)) + 0.01;
        if (d - d1).abs() < 0.01 {
            d = jmin(d, progress) + 0.01;
            move_entity_by_piston(l, dir, e, d, opposite);
        }
    }
}

// PistonMovingBlockEntity.fixEntityWithinPistonBase for the client player.
fn fix_player_within_piston_base(l: &Level, pos: Pos, p: &mut crate::player::Player, dir: Dir, progress: f64) {
    let bounding_box = p.bb;
    let aabb = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0).mv(pos.x() as f64, pos.y() as f64, pos.z() as f64);
    if bounding_box.intersects(&aabb) {
        let opposite = dir.opposite();
        let mut d = get_movement(&aabb, opposite, &bounding_box) + 0.01;
        let d1 = get_movement(&aabb, opposite, &intersect(&bounding_box, &aabb)) + 0.01;
        if (d - d1).abs() < 0.01 {
            d = jmin(d, progress) + 0.01;
            move_player_by_piston(l, p, dir, d, opposite);
        }
    }
}

// The entity a push takes out of the level to move it: one the lookup found
// that is gone has been removed by another's push, which no push does.
fn take(l: &mut Level, id: EntityId) -> Box<entity::Entity> {
    match l.entities.all.remove(&id) {
        Some(e) => e,
        None => crate::level::refuse("an entity left the level while a moving piston pushed the entities in its way"),
    }
}

// The entity goes back into the level unless it was removed.
fn put_back(l: &mut Level, e: Box<entity::Entity>) {
    if !e.removed {
        l.entities.all.insert(e.id, e);
    }
}

// PistonMovingBlockEntity.moveCollidedEntities.
fn move_collided_entities(l: &mut Level, pos: Pos, partial_tick: f32, piston: &Moving) {
    let movement_direction = piston.movement_direction();
    let d = (partial_tick - piston.progress) as f64;
    let related = collision_related_state(l, piston);
    let collision_shape = state_shape(l, related, pos);
    if collision_shape.is_empty() {
        return;
    }
    let bounds = collision_shape.bounds();
    let aabb = move_by_position_and_progress(pos, &Aabb::new(bounds[0], bounds[1], bounds[2], bounds[3], bounds[4], bounds[5]), piston);
    let area = minmax(&movement_area(&aabb, movement_direction, d), &aabb);
    // Level.getEntities(null, area) with EntitySelector.NO_SPECTATORS.
    let entities = entity::entities_in(l, None, None, Only::Any, &area, |_| true);
    let player_in = l.player.as_ref().is_some_and(|p| p.bb.intersects(&area));
    if entities.is_empty() && !player_in {
        return;
    }
    let list: Vec<Aabb> = collision_shape.boxes().iter().map(|b| Aabb::new(b[0], b[1], b[2], b[3], b[4], b[5])).collect();
    let is_slime_block = l.block(piston.moved_state) == piston::slime_block(l);
    for id in entities {
        let mut e = take(l, id);
        // Entity.getPistonPushReaction is NORMAL for every entity here.
        if is_slime_block {
            // The slime block sets the entity's motion along the movement's axis.
            let delta = e.delta;
            let (sx, sy, sz) = movement_direction.offset();
            let (mut d1, mut d2, mut d3) = (delta.x, delta.y, delta.z);
            match movement_direction.axis() {
                crate::dir::Axis::X => d1 = sx as f64,
                crate::dir::Axis::Y => d2 = sy as f64,
                crate::dir::Axis::Z => d3 = sz as f64,
            }
            e.delta = Vec3::new(d1, d2, d3);
        }
        let mut d4 = 0.0;
        for b in &list {
            let movement = movement_area(&move_by_position_and_progress(pos, b, piston), movement_direction, d);
            let bounding_box = e.bb;
            if movement.intersects(&bounding_box) {
                d4 = jmax(d4, get_movement(&movement, movement_direction, &bounding_box));
                if d4 >= d {
                    break;
                }
            }
        }
        if !(d4 <= 0.0) {
            d4 = jmin(d4, d) + 0.01;
            move_entity_by_piston(l, movement_direction, &mut e, d4, movement_direction);
            if !piston.extending && piston.is_source_piston {
                fix_entity_within_piston_base(l, pos, &mut e, movement_direction, d);
            }
        }
        put_back(l, e);
    }
    // The client player, a LocalPlayer (a slime block sets its motion too).
    if player_in {
        let mut p = l.player.take().expect("the player of the level");
        if is_slime_block {
            let delta = p.delta;
            let (sx, sy, sz) = movement_direction.offset();
            let (mut d1, mut d2, mut d3) = (delta.x, delta.y, delta.z);
            match movement_direction.axis() {
                crate::dir::Axis::X => d1 = sx as f64,
                crate::dir::Axis::Y => d2 = sy as f64,
                crate::dir::Axis::Z => d3 = sz as f64,
            }
            p.delta = Vec3::new(d1, d2, d3);
        }
        let mut d4 = 0.0;
        for b in &list {
            let movement = movement_area(&move_by_position_and_progress(pos, b, piston), movement_direction, d);
            let bounding_box = p.bb;
            if movement.intersects(&bounding_box) {
                d4 = jmax(d4, get_movement(&movement, movement_direction, &bounding_box));
                if d4 >= d {
                    break;
                }
            }
        }
        if !(d4 <= 0.0) {
            d4 = jmin(d4, d) + 0.01;
            move_player_by_piston(l, &mut p, movement_direction, d4, movement_direction);
            if !piston.extending && piston.is_source_piston {
                fix_player_within_piston_base(l, pos, &mut p, movement_direction, d);
            }
        }
        l.player = Some(p);
    }
}

// PistonMovingBlockEntity.moveStuckEntities: what stands on honey is carried.
fn move_stuck_entities(l: &mut Level, pos: Pos, partial_tick: f32, piston: &Moving) {
    // isStickyForEntities.
    if l.block(piston.moved_state) != piston::honey_block(l) {
        return;
    }
    let movement_direction = piston.movement_direction();
    if !movement_direction.is_horizontal() {
        return;
    }
    let d = state_shape(l, piston.moved_state, pos).max_along(crate::dir::Axis::Y);
    let aabb = move_by_position_and_progress(pos, &Aabb::new(0.0, d, 0.0, 1.0, 1.5000010000000001, 1.0), piston);
    let d1 = (partial_tick - piston.progress) as f64;
    // matchesStickyCritera: pushed normally, on the ground, and held up by
    // this block or within the box horizontally.
    let stuck = entity::entities_in(l, None, None, Only::Any, &aabb, |e| {
        e.on_ground && (e.main_supporting_block == Some(pos) || e.pos.x >= aabb.min_x && e.pos.x <= aabb.max_x && e.pos.z >= aabb.min_z && e.pos.z <= aabb.max_z)
    });
    for id in stuck {
        let mut e = take(l, id);
        move_entity_by_piston(l, movement_direction, &mut e, d1, movement_direction);
        put_back(l, e);
    }
    // The client player on honey is carried too.
    if l.player.as_ref().is_some_and(|p| p.bb.intersects(&aabb) && p.on_ground && (p.supporting_block() == Some(pos) || p.pos.x >= aabb.min_x && p.pos.x <= aabb.max_x && p.pos.z >= aabb.min_z && p.pos.z <= aabb.max_z)) {
        let mut p = l.player.take().expect("the player of the level");
        move_player_by_piston(l, &mut p, movement_direction, d1, movement_direction);
        l.player = Some(p);
    }
}

// The entity's saved data (PistonMovingBlockEntity.saveAdditional), keys in
// order: the moved block state as BlockState.CODEC writes it, the direction
// as its 3d data value, and the progress at the last tick.
pub fn saved(moving: &Moving, reg: &Registry) -> (&'static str, String) {
    let mut properties = reg.values(moving.moved_state);
    properties.sort();
    let mut state = format!("{{Name:\"{}\"", reg.blocks[reg.block(moving.moved_state) as usize].key);
    if !properties.is_empty() {
        let list: Vec<String> = properties.iter().map(|(name, value)| format!("{name}:\"{value}\"")).collect();
        state.push_str(&format!(",Properties:{{{}}}", list.join(",")));
    }
    state.push('}');
    let flag = |value: bool| if value { "1b" } else { "0b" };
    (
        "minecraft:piston",
        format!(
            "{{blockState:{state},components:{{}},extending:{},facing:{}b,progress:{:?}f,source:{}}}",
            flag(moving.extending),
            moving.direction as i32,
            moving.progress_o,
            flag(moving.is_source_piston)
        ),
    )
}
