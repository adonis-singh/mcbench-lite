// What fluids do to an entity (Entity.updateInWaterStateAndDoFluidPushing and
// updateFluidHeightAndDoFluidPushing, updateFluidOnEyes, doWaterSplashEffect,
// the bubble columns' pull, lava's fire) for an entity that no vehicle,
// rider, rain, powder snow or player acts on, in the order Java does them.
// The sounds and particles reach nobody but still draw their random numbers.

use super::Entity;
use crate::fluid;
use crate::dir::Pos;
use crate::level::Level;
use crate::phys::{self, jmax, Vec3};
use crate::registry::EMPTY_FLUID;
use crate::world::{HEIGHT, MIN_Y};

// The index of a fluid tag in Entity.fluidHeight: water, lava.
const WATER: usize = 0;
const LAVA: usize = 1;

// Entity.touchingUnloadedChunk: the box grown by a block, in chunks.
fn touching_unloaded_chunk(l: &Level, e: &Entity) -> bool {
    let grown = e.bb.inflate(1.0, 1.0, 1.0);
    let (min_x, min_z) = (phys::floor(grown.min_x), phys::floor(grown.min_z));
    let (max_x, max_z) = (phys::ceil(grown.max_x), phys::ceil(grown.max_z));
    for cz in (min_z >> 4)..=(max_z >> 4) {
        for cx in (min_x >> 4)..=(max_x >> 4) {
            if !l.world.has_chunk(cx, cz) {
                return true;
            }
        }
    }
    false
}

// Entity.updateFluidHeightAndDoFluidPushing(tag, flowScale) for an entity
// that is not a player and is pushed by fluid.
fn update_fluid_height_and_do_fluid_pushing(l: &mut Level, e: &mut Entity, tag: usize, flow_scale: f64) -> bool {
    if touching_unloaded_chunk(l, e) {
        return false;
    }
    let bb = e.bb.deflate(0.001);
    let min_x = phys::floor(bb.min_x);
    let min_y = (MIN_Y >> 4 << 4).max(phys::floor(bb.min_y));
    let min_z = phys::floor(bb.min_z);
    let max_x = phys::ceil(bb.max_x) - 1;
    let max_y = ((MIN_Y + HEIGHT >> 4) - 1 << 4 | 15).min(phys::ceil(bb.max_y) - 1);
    let max_z = phys::ceil(bb.max_z) - 1;
    // The push vector, none while it is Vec3.ZERO itself.
    let mut push: Option<Vec3> = None;
    let mut total_pushes = 0.0f64;
    let mut max_height_diff = 0.0f64;
    let mut in_fluid = false;
    for x in min_x..=max_x {
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                let pos = Pos::new(x, y, z);
                let state = l.world.get(pos);
                let fluid_state = l.fluid_of(state);
                if fluid_state == EMPTY_FLUID {
                    continue;
                }
                let kind = l.fluid_type(fluid_state);
                if !(if tag == WATER { kind.is_water() } else { kind.is_lava() }) {
                    continue;
                }
                if tag == LAVA {
                    e.last_lava_contact = Some(pos);
                }
                let height = (y as f32 + fluid::height(l, fluid_state, pos)) as f64;
                let diff = height - bb.min_y;
                if !(diff < 0.0) {
                    in_fluid = true;
                    max_height_diff = jmax(max_height_diff, diff);
                    total_pushes += 1.0;
                    let flow = fluid::get_flow(l, pos, fluid_state);
                    let step = if max_height_diff < 0.4 { flow.scale(max_height_diff) } else { flow };
                    push = Some(push.unwrap_or(Vec3::ZERO).add_vec(step));
                }
            }
        }
    }
    e.fluid_height[tag] = max_height_diff;
    let Some(push) = push else { return in_fluid };
    let mut push = push.scale(1.0 / total_pushes);
    let current = e.delta;
    push = push.normalize();
    push = push.scale(flow_scale);
    if current.x.abs() < 0.003 && current.z.abs() < 0.003 && push.length() < 0.0045000000000000005 {
        push = push.normalize().scale(0.0045000000000000005);
    }
    e.delta = current.add_vec(push);
    true
}

// Entity.doWaterSplashEffect: the sound's pitch and the particles' offsets
// draw from the shared random. ExperienceOrb's does nothing.
fn do_water_splash_effect(l: &mut Level, e: &Entity) {
    if matches!(e.kind, super::Kind::Orb(_)) {
        return;
    }
    // playSound(sound, min, 1.0F + (nextFloat() - nextFloat()) * 0.4F).
    let first = l.shared_random.next_float();
    let second = l.shared_random.next_float();
    let _pitch = 1.0f32 + (first - second) * 0.4f32;
    let width = e.width;
    // for (i = 0; i < 1.0F + width * 20.0F; i++): two offsets and the
    // bubble's downward speed; then two offsets for the splash.
    let mut i = 0;
    while (i as f32) < 1.0f32 + width * 20.0f32 {
        l.shared_random.next_double();
        l.shared_random.next_double();
        l.shared_random.next_double();
        i += 1;
    }
    let mut i = 0;
    while (i as f32) < 1.0f32 + width * 20.0f32 {
        l.shared_random.next_double();
        l.shared_random.next_double();
        i += 1;
    }
    // gameEvent(SPLASH).
    let source = crate::game_event::Source::of(l, e);
    let event = crate::game_event!(l, "splash");
    l.game_event(event, e.pos, crate::game_event::Context::of_entity(Some(source)));
}

// Entity.updateInWaterStateAndDoFluidPushing: whether the entity is in water
// or in lava.
pub fn update_in_water_state_and_do_fluid_pushing(l: &mut Level, e: &mut Entity) -> bool {
    e.fluid_height = [0.0, 0.0];
    update_in_water_state_and_do_water_current_pushing(l, e);
    let lava = update_fluid_height_and_do_fluid_pushing(l, e, LAVA, 0.0023333333333333335);
    e.was_touching_water || lava
}

// Entity.updateInWaterStateAndDoWaterCurrentPushing.
pub fn update_in_water_state_and_do_water_current_pushing(l: &mut Level, e: &mut Entity) {
    if update_fluid_height_and_do_fluid_pushing(l, e, WATER, 0.014) {
        if !e.was_touching_water && !e.first_tick {
            do_water_splash_effect(l, e);
        }
        e.fall_distance = 0.0;
        e.was_touching_water = true;
    } else {
        e.was_touching_water = false;
    }
}

// Entity.updateFluidOnEyes: wasEyeInWater, then the fluid at the eyes if its
// surface is above them (fluidOnEyes holds its tags).
pub fn update_fluid_on_eyes(l: &mut Level, e: &mut Entity) {
    e.was_eye_in_water = e.fluid_on_eyes[WATER];
    e.fluid_on_eyes = [false, false];
    let eye_y = e.eye_y();
    let eye = Pos::new(phys::floor(e.pos.x), phys::floor(eye_y), phys::floor(e.pos.z));
    let fluid_state = l.fluid_state(eye);
    // fluidState.getHeight reads the fluid above.
    let height = fluid::height(l, fluid_state, eye);
    let d = (eye.y() as f32 + height) as f64;
    if d > eye_y {
        match l.fluid_type(fluid_state) {
            crate::registry::Fluid::FlowingWater | crate::registry::Fluid::Water => e.fluid_on_eyes[WATER] = true,
            crate::registry::Fluid::FlowingLava | crate::registry::Fluid::Lava => e.fluid_on_eyes[LAVA] = true,
            crate::registry::Fluid::Empty => {}
        }
    }
}

// Entity.getFluidHeight(water), getFluidHeight(lava).
pub fn water_height(e: &Entity) -> f64 {
    e.fluid_height[WATER]
}

pub fn lava_height(e: &Entity) -> f64 {
    e.fluid_height[LAVA]
}

// Entity.isInLava.
pub fn is_in_lava(e: &Entity) -> bool {
    !e.first_tick && e.fluid_height[LAVA] > 0.0
}

// Entity.clearFire.
pub fn clear_fire(e: &mut Entity) {
    e.remaining_fire_ticks = e.remaining_fire_ticks.min(0);
}

// Entity.igniteForTicks: the fire, then clearFreeze (freezeLocked is never set).
pub fn ignite_for_ticks(e: &mut Entity, ticks: i32) {
    if e.remaining_fire_ticks < ticks {
        e.remaining_fire_ticks = ticks;
    }
    e.ticks_frozen = 0;
}

// Entity.lavaIgnite for an entity that is not living: igniteForSeconds(15, false).
pub fn lava_ignite(e: &mut Entity, fire_immune: bool) {
    if !fire_immune {
        ignite_for_ticks(e, phys::floor(15.0f32 as f64 * 20.0f32 as f64));
    }
}

// Entity.handleOnAboveBubbleColumn: the pull on an entity above a bubble
// column, and the particles' random positions (two rounds of four draws from
// the level's random).
pub fn on_above_bubble_column(l: &mut Level, e: &mut Entity, downwards: bool) {
    let dm = e.delta;
    let y = if downwards { jmax(-0.9, dm.y - 0.03) } else { crate::phys::jmin(1.8, dm.y + 0.1) };
    e.delta = Vec3::new(dm.x, y, dm.z);
    for _ in 0..8 {
        l.random.next_double();
    }
}

// Entity.handleOnInsideBubbleColumn.
pub fn on_inside_bubble_column(e: &mut Entity, downwards: bool) {
    let dm = e.delta;
    let y = if downwards { jmax(-0.3, dm.y - 0.03) } else { crate::phys::jmin(0.7, dm.y + 0.06) };
    e.delta = Vec3::new(dm.x, y, dm.z);
    e.fall_distance = 0.0;
}
