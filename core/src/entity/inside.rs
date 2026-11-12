// What the blocks an entity is inside do to it: Entity.checkInsideBlocks'
// InsideBlockEffectApplier.StepBasedCollector and its effects
// (InsideBlockEffectType), BlockState.getEntityInsideCollisionShape, and the
// dispatch to each class's entityInside (EntityInsideBlockEvent has no
// listener in this level, so every class goes on to its own code).

use super::damage::Dmg;
use super::{fluid, item, Entity, Kind};
use crate::dir::Pos;
use crate::level::{refuse, Level};
use crate::phys::Aabb;
use crate::registry::State;
use crate::shape_ops;
use std::borrow::Cow;

// InsideBlockEffectType.values(): FREEZE, CLEAR_FREEZE, FIRE_IGNITE,
// LAVA_IGNITE, EXTINGUISH.
pub const FREEZE: usize = 0;
pub const CLEAR_FREEZE: usize = 1;
pub const FIRE_IGNITE: usize = 2;
pub const LAVA_IGNITE: usize = 3;
pub const EXTINGUISH: usize = 4;

// An effect the collector holds for the entity: InsideBlockEffectType's own,
// at the position of the block that applied it, or what a block ran before
// or after one (the lambdas of runBefore and runAfter).
#[derive(Clone, Copy)]
pub enum Effect {
    // Entity.setIsInPowderSnow(true) and one more tick frozen.
    Freeze,
    // Entity.clearFreeze.
    ClearFreeze,
    // BaseFireBlock.fireIgnite(entity, pos).
    FireIgnite(Pos),
    // Entity.lavaIgnite(pos).
    LavaIgnite,
    // Entity.clearFire.
    Extinguish,
    // Entity.lavaHurt(pos).
    LavaHurt(Pos),
    // BaseFireBlock's runAfter(FIRE_IGNITE): entity.hurt(inFire, fireDamage).
    FireHurt(f32),
    // PowderSnowBlock's runBefore(EXTINGUISH): a burning entity breaks it.
    PowderSnowBreak(Pos),
    // LayeredCauldronBlock's runBefore(EXTINGUISH): a burning entity lowers the
    // level of the cauldron (the state it was in when the entity entered).
    CauldronExtinguish(Pos, State),
}

// InsideBlockEffectApplier.StepBasedCollector.
pub struct Applier {
    in_step: [Option<Effect>; 5],
    before: [Vec<Effect>; 5],
    after: [Vec<Effect>; 5],
    final_effects: Vec<Effect>,
    last_step: i32,
    current: Pos,
}

impl Default for Applier {
    fn default() -> Applier {
        Applier { in_step: [None; 5], before: Default::default(), after: Default::default(), final_effects: Vec::new(), last_step: -1, current: Pos::new(0, 0, 0) }
    }
}

impl Applier {
    pub fn advance_step(&mut self, step: i32, pos: Pos) {
        self.current = pos;
        if self.last_step != step {
            self.last_step = step;
            self.flush_step();
        }
    }

    // flushStep: per effect type in order, what was run before it, the effect
    // of the step, what was run after it.
    fn flush_step(&mut self) {
        for kind in 0..5 {
            let before = std::mem::take(&mut self.before[kind]);
            self.final_effects.extend(before);
            if let Some(effect) = self.in_step[kind].take() {
                self.final_effects.push(effect);
            }
            let after = std::mem::take(&mut self.after[kind]);
            self.final_effects.extend(after);
        }
    }

    // apply(type): the effect is recorded with the current position.
    pub fn apply(&mut self, kind: usize) {
        self.in_step[kind] = Some(match kind {
            FREEZE => Effect::Freeze,
            CLEAR_FREEZE => Effect::ClearFreeze,
            FIRE_IGNITE => Effect::FireIgnite(self.current),
            LAVA_IGNITE => Effect::LavaIgnite,
            EXTINGUISH => Effect::Extinguish,
            _ => unreachable!("no such effect type"),
        });
    }

    pub fn run_before(&mut self, kind: usize, effect: Effect) {
        self.before[kind].push(effect);
    }

    pub fn run_after(&mut self, kind: usize, effect: Effect) {
        self.after[kind].push(effect);
    }

    pub fn apply_and_clear(&mut self, l: &mut Level, e: &mut Entity) {
        self.flush_step();
        for effect in std::mem::take(&mut self.final_effects) {
            if !super::is_alive(e) {
                break;
            }
            apply_effect(l, e, effect);
        }
        self.last_step = -1;
    }
}

// Entity.canFreeze: no entity of this level is of a freeze immune type, and a
// living one (the armor stand) is not wearing freeze immune armor.
fn can_freeze(l: &Level, e: &Entity) -> bool {
    match e.kind {
        Kind::ArmorStand(_) => super::armor_stand::can_freeze(l, e),
        _ => true,
    }
}

fn apply_effect(l: &mut Level, e: &mut Entity, effect: Effect) {
    match effect {
        // (freezeLocked is Paper.FreezeLock, which an import refuses.)
        Effect::Freeze => {
            e.in_powder_snow = true;
            if can_freeze(l, e) {
                e.ticks_frozen = super::armor_stand::FREEZE_TICKS.min(e.ticks_frozen + 1);
            }
        }
        Effect::ClearFreeze => e.ticks_frozen = 0,
        Effect::FireIgnite(_) => fire_ignite(l, e),
        Effect::LavaIgnite => {
            let immune = item::fire_immune(l, e);
            fluid::lava_ignite(e, immune);
        }
        Effect::Extinguish => fluid::clear_fire(e),
        Effect::LavaHurt(pos) => super::lava_hurt(l, e, pos),
        Effect::FireHurt(damage) => {
            super::hurt(l, e, Dmg::InFire, damage);
        }
        Effect::PowderSnowBreak(pos) => {
            // Entity.isOnFire, mayInteract (true for all but a player), and the
            // game rule mobGriefing (on): EntityChangeBlockEvent has no listener.
            if item::is_on_fire(l, e) {
                l.destroy_block_by(pos, false, 512, None);
            }
        }
        Effect::CauldronExtinguish(pos, state) => {
            if item::is_on_fire(l, e) && crate::blocks::cauldron::handle_entity_on_fire_inside(l, state, pos) {
                fluid::clear_fire(e);
            }
        }
    }
}

// BaseFireBlock.fireIgnite(entity, pos): an entity that burns is not set alight
// again unless it was out (a player's extra fire does not reach here), and
// EntityCombustByBlockEvent has no listener: igniteForSeconds(8.0F, false).
fn fire_ignite(l: &mut Level, e: &mut Entity) {
    if item::fire_immune(l, e) {
        return;
    }
    if e.remaining_fire_ticks < 0 {
        e.remaining_fire_ticks += 1;
    }
    if e.remaining_fire_ticks >= 0 {
        fluid::ignite_for_ticks(e, crate::phys::floor(8.0f32 as f64 * 20.0f32 as f64));
    }
}

// BlockState.getEntityInsideCollisionShape for the classes that override it:
// the shape the entity must collide with to be inside the block. None is
// Shapes.block() (the default), which is always entered.
pub fn inside_shape(l: &Level, e: &Entity, state: State, pos: Pos) -> Option<Cow<'static, crate::registry::Shape>> {
    // The classes that override it all override entityInside.
    if !crate::blocks::overrides_entity_inside(l, state) {
        return None;
    }
    match l.reg.blocks[l.block(state) as usize].class.as_str() {
        // BlockState.getShape: the outline, which the dump does not hold.
        "TripWireBlock" => Some(Cow::Owned(crate::blocks::tripwire::outline_shape(l, state))),
        "EndPortalBlock" => Some(Cow::Owned(shape_ops::block_box(0.0, 6.0, 0.0, 16.0, 12.0, 16.0))),
        // The cauldron's own collision shape and what it holds.
        "LayeredCauldronBlock" => Some(Cow::Owned(crate::blocks::cauldron::layered_inside_shape(l, state))),
        "LavaCauldronBlock" => Some(Cow::Owned(crate::blocks::cauldron::lava_inside_shape(l, state))),
        // The collision shape for the entity, or the block where it is empty.
        "PowderSnowBlock" => {
            let shape = crate::collision::shape_of(l, state, pos, super::collide::context(l, e));
            // (Shapes.block() itself, as the default is, is always entered.)
            if shape.is_empty() || crate::collision::is_shapes_block(&shape) {
                None
            } else {
                Some(shape)
            }
        }
        _ => None,
    }
}

// Entity.collidedWithShapeMovingFrom(from, to, shape.move(pos).toAabbs()).
pub fn collided_with_shape(e: &Entity, from: crate::phys::Vec3, to: crate::phys::Vec3, shape: &crate::registry::Shape, pos: Pos) -> bool {
    let boxes: Vec<Aabb> = shape.boxes().iter().map(|b| Aabb::new(b[0] + pos.x() as f64, b[1] + pos.y() as f64, b[2] + pos.z() as f64, b[3] + pos.x() as f64, b[4] + pos.y() as f64, b[5] + pos.z() as f64)).collect();
    e.make_bounding_box(from).collided_along_vector(to.subtract(from), &boxes)
}

// A block that has an entityInside the port does not act on for the kinds of
// entity that reach it refuses.
#[cold]
pub fn unported(l: &Level, state: State) -> ! {
    refuse(format!("{}.entityInside ({})", l.reg.blocks[l.block(state) as usize].class, l.reg.describe(state)))
}
