// VehicleEntity: what minecarts and boats share (the hurt time, direction and
// damage; VehicleEntity.hurtServer, destroy and kill), and the pushing of
// Entity.push(Entity).

use super::damage::Dmg;
use super::item;
use super::{minecart, boat, remove, Entity, Kind};
use crate::level::Level;
use crate::loot::ItemStack;

// VehicleEntity's synched hurt time (DATA_ID_HURT), direction (DATA_ID_HURTDIR,
// 1 or -1) and damage (DATA_ID_DAMAGE).
#[derive(Clone, Copy, Debug)]
pub struct Vehicle {
    pub hurt_time: i32,
    pub hurt_dir: i32,
    pub damage: f32,
}

impl Vehicle {
    pub fn new() -> Vehicle {
        Vehicle { hurt_time: 0, hurt_dir: 1, damage: 0.0 }
    }

    // The part of the tick that wears the hurt time and the damage down.
    pub fn wear_down(&mut self) {
        if self.hurt_time > 0 {
            self.hurt_time -= 1;
        }
        if self.damage > 0.0f32 {
            self.damage -= 1.0f32;
        }
    }
}

pub fn of(e: &Entity) -> &Vehicle {
    match &e.kind {
        Kind::Minecart(c) => &c.vehicle,
        Kind::Boat(b) => &b.vehicle,
        _ => panic!("not a vehicle"),
    }
}

pub fn of_mut(e: &mut Entity) -> &mut Vehicle {
    match &mut e.kind {
        Kind::Minecart(c) => &mut c.vehicle,
        Kind::Boat(b) => &mut b.vehicle,
        _ => panic!("not a vehicle"),
    }
}

// Entity.isInvulnerableToBase.
fn invulnerable_to_base(l: &Level, e: &Entity, dmg: Dmg) -> bool {
    e.removed || e.invulnerable || dmg.is_fire() && item::fire_immune(l, e)
}

// VehicleEntity.hurtServer (the damage source has no causing entity).
pub fn hurt(l: &mut Level, e: &mut Entity, dmg: Dmg, amount: f32) -> bool {
    if e.removed {
        return true;
    }
    if invulnerable_to_base(l, e, dmg) {
        return false;
    }
    {
        let v = of_mut(e);
        v.hurt_dir = -v.hurt_dir;
        v.hurt_time = 10;
        v.damage = v.damage + amount * 10.0f32;
    }
    let event = crate::game_event!(l, "entity_damage");
    l.game_event(event, e.pos, crate::game_event::Context::of_entity(None));
    let source_destroys = matches!(&e.kind, Kind::Minecart(c) if minecart::should_source_destroy(c.class, dmg));
    if !(!(of(e).damage > 40.0f32) && !source_destroys) {
        match e.kind {
            Kind::Minecart(_) => minecart::destroy(l, e, dmg),
            Kind::Boat(_) => boat::destroy(l, e),
            _ => unreachable!(),
        }
    }
    true
}

// VehicleEntity.destroy(level, dropItem): the vehicle is killed and its item
// drops (the game rule doEntityDrops is on).
pub fn destroy_with_item(l: &mut Level, e: &mut Entity, key: &str) {
    kill(l, e);
    let mut stack = ItemStack::new(l.reg.item_by_key[key], 1);
    // itemStack.set(CUSTOM_NAME, getCustomName()).
    if let Some(name) = e.extra.get("CustomName") {
        stack.set_component(l.reg, "minecraft:custom_name", name.clone());
    }
    item::spawn_with_default_delay(l, e.pos.x + 0.0, e.pos.y + 0.0, e.pos.z + 0.0, stack);
}

// Entity.kill(level): removed as killed, and ENTITY_DIE.
pub fn kill(l: &mut Level, e: &mut Entity) {
    remove(l, e);
    let event = crate::game_event!(l, "entity_die");
    l.game_event(event, e.pos, crate::game_event::Context::of_entity(Some(crate::game_event::Source::of(l, e))));
}

// Entity.push(x, y, z): the motion changes.
pub fn push_by(e: &mut Entity, x: f64, y: f64, z: f64) {
    e.delta = e.delta.add(x, y, z);
    e.has_impulse = true;
}

// Entity.push(entity): `this` and `entity` push each other away.
pub fn push_entity(this: &mut Entity, entity: &mut Entity) {
    // (isPassengerOfSameVehicle is false for entities that ride nothing.)
    if entity.no_physics || this.no_physics {
        return;
    }
    let mut d = entity.pos.x - this.pos.x;
    let mut d1 = entity.pos.z - this.pos.z;
    // Mth.absMax: Math.max of the absolute values.
    let mut max = crate::phys::jmax(d.abs(), d1.abs());
    if max >= 0.01f32 as f64 {
        max = max.sqrt();
        d /= max;
        d1 /= max;
        let mut d2 = 1.0 / max;
        if d2 > 1.0 {
            d2 = 1.0;
        }
        d *= d2;
        d1 *= d2;
        d *= 0.05f32 as f64;
        d1 *= 0.05f32 as f64;
        // isVehicle() is false: pushable entities are pushed.
        if super::collide::is_pushable(this) {
            push_by(this, -d, 0.0, -d1);
        }
        if super::collide::is_pushable(entity) {
            push_by(entity, d, 0.0, d1);
        }
    }
}
