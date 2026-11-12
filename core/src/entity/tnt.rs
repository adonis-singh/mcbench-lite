// PrimedTnt: priming (TntBlock.prime and wasExploded), its tick and its
// explosion. Paper's configuration is at its defaults: Spigot's
// max-tnt-per-tick 100 (Level.spigotConfig.currentPrimedTnt, reset by
// Level.tickBlockEntities), fixes.tnt-entity-height-nerf disabled,
// fixes.prevent-tnt-from-moving-in-water off, and the game rule tntExplodes on.
//
// Events the Bukkit layer calls (TNTPrimeEvent, ExplosionPrimeEvent) have no
// listener: their results stand (an explosion of radius 4.0, no fire).

use super::fluid;
use super::motion::{self, set_pos};
use super::{add_fresh_entity, remove, Entity, Kind};
use crate::dir::Pos;
use crate::level::Level;
use crate::phys::Vec3;
use crate::registry::State;

// PrimedTnt's fields (its owner is none; it has used no portal).
pub struct TntData {
    pub fuse: i32,
    pub state: State,
    pub explosion_power: f32,
}

// Spigot's max-tnt-per-tick.
const MAX_TNT_TICKS_PER_TICK: i32 = 100;

fn data(e: &Entity) -> &TntData {
    match &e.kind {
        Kind::Tnt(d) => d,
        _ => panic!("not a primed TNT"),
    }
}

fn data_mut(e: &mut Entity) -> &mut TntData {
    match &mut e.kind {
        Kind::Tnt(d) => d,
        _ => panic!("not a primed TNT"),
    }
}

// PrimedTnt(level, x, y, z, null): not added to the level yet.
fn new_primed(l: &mut Level, x: f64, y: f64, z: f64) -> Box<Entity> {
    let state = l.reg.default_state(l.reg.block_key("minecraft:tnt"));
    // EntityType.TNT: 0.98 by 0.98, eye height 0.15.
    let mut e = Box::new(Entity::new(l, Kind::Tnt(TntData { fuse: 80, state, explosion_power: 4.0f32 }), 0.98f32, 0.98f32, 0.15f32));
    set_pos(l, &mut e, x, y, z);
    let d = l.shared_random.next_double() * ((std::f64::consts::PI * 2.0) as f32) as f64;
    e.delta = Vec3::new(-crate::jmath::sin(d) * 0.02, 0.2f32 as f64, -crate::jmath::cos(d) * 0.02);
    e.xo = x;
    e.yo = y;
    e.zo = z;
    e
}

// TntBlock.prime(level, pos, event): the TNT entity above the block's center.
pub fn prime(l: &mut Level, pos: Pos) -> bool {
    let e = new_primed(l, pos.x() as f64 + 0.5, pos.y() as f64, pos.z() as f64 + 0.5);
    add_fresh_entity(l, e);
    // The sound reaches no one; PRIME_FUSE has no entity (the player).
    let event = crate::game_event!(l, "prime_fuse");
    l.game_event_at(None, event, pos);
    true
}

// The TNT block item's dispenser behaviour: a primed TNT above the center of
// the block in front, and the entity placed there.
pub fn dispensed(l: &mut Level, pos: Pos) {
    let e = new_primed(l, pos.x() as f64 + 0.5, pos.y() as f64, pos.z() as f64 + 0.5);
    add_fresh_entity(l, e);
    let event = crate::game_event!(l, "entity_place");
    l.game_event_at(None, event, pos);
}

// TntBlock.wasExploded, for an explosion with no owner.
pub fn was_exploded(l: &mut Level, pos: Pos) {
    let mut e = new_primed(l, pos.x() as f64 + 0.5, pos.y() as f64, pos.z() as f64 + 0.5);
    let fuse = data(&e).fuse;
    data_mut(&mut e).fuse = (l.random.next_int_bound(fuse / 4) + fuse / 8) as i16 as i32;
    add_fresh_entity(l, e);
}

// PrimedTnt.tick.
pub fn tick(l: &mut Level, e: &mut Entity) {
    l.current_primed_tnt += 1;
    if l.current_primed_tnt > MAX_TNT_TICKS_PER_TICK {
        return;
    }
    motion::handle_portal(e);
    // applyGravity: 0.04.
    e.delta = e.delta.add(0.0, -0.04, 0.0);
    motion::move_entity(l, e, e.delta);
    motion::apply_effects_from_blocks(l, e);
    e.delta = e.delta.scale(0.98);
    if e.on_ground {
        e.delta = e.delta.multiply(0.7, -0.5, 0.7);
    }
    let fuse = data(e).fuse - 1;
    data_mut(e).fuse = fuse;
    if fuse <= 0 {
        explode(l, e);
        remove(l, e);
    } else {
        fluid::update_in_water_state_and_do_fluid_pushing(l, e);
    }
}

// PrimedTnt.explode: ExplosionPrimeEvent's radius is the TNT's power and
// it makes no fire; the explosion is at the TNT's x, z and 1/16 of its height up.
fn explode(l: &mut Level, e: &Entity) {
    let y = e.pos.y + e.height as f64 * 0.0625;
    crate::explosion::explode(l, e, Vec3::new(e.pos.x, y, e.pos.z), data(e).explosion_power, false);
}
