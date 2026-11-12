// What entities do to each other's movement: CollisionUtil.getEntityHardCollisions
// and EntityGetter.getEntityCollisions (the boxes of the entities an entity
// collides with), and the predicates behind them (Entity.canCollideWith,
// canBeCollidedWith, isCollidable, isPushable). Paper's configuration is at
// its defaults (collisions.only-players-collide false).

use super::{Entity, Kind};
use crate::collision::{Ctx, EntityCtx};
use crate::level::Level;
use crate::phys::Aabb;

// CollisionContext.of(entity). (AbstractMinecart.useExperimentalMovement is
// off: the minecart improvements are an experimental feature this world does
// not enable.)
pub fn context(l: &Level, e: &Entity) -> Ctx {
    // PowderSnowBlock.canEntityWalkOnPowderSnow: the powder snow walkable
    // types are mobs; a living entity (the armor stand) wearing leather boots.
    let walks_on_powder_snow = match &e.kind {
        Kind::ArmorStand(data) => {
            let feet = &data.equipment[super::armor_stand::FEET];
            feet.count > 0 && feet.item == l.reg.item_by_key["minecraft:leather_boots"]
        }
        _ => false,
    };
    Ctx::Entity(EntityCtx { bottom: e.pos.y, fall_distance: e.fall_distance, falling_block: matches!(e.kind, Kind::Falling(_)), walks_on_powder_snow, descending: false })
}

// Entity.isCollidable(false) (Entity.isPushable is the same).
pub fn is_collidable(e: &Entity) -> bool {
    matches!(e.kind, Kind::Minecart(_) | Kind::Boat(_))
}

pub fn is_pushable(e: &Entity) -> bool {
    is_collidable(e)
}

// Entity.canBeCollidedWith(entity): what a minecart, a boat... is to an entity
// that moves into it.
pub fn can_be_collided_with(e: &Entity) -> bool {
    matches!(e.kind, Kind::Boat(_))
}

// moonrise$isHardColliding: a boat, a minecart, a shulker, a happy ghast, or an
// entity that can be collided with.
pub fn is_hard_colliding(kind: &Kind) -> bool {
    matches!(kind, Kind::Minecart(_) | Kind::Boat(_))
}

// Entity.canCollideWith(other) of `me`: the vehicles' version for a minecart or
// a boat (AbstractBoat.canVehicleCollide), else the entity's. Neither rides
// anything here.
pub fn can_collide_with(me: &Entity, other: &Entity) -> bool {
    match me.kind {
        Kind::Minecart(_) | Kind::Boat(_) => can_be_collided_with(other) || is_pushable(other),
        _ => can_be_collided_with(other),
    }
}

// Entity.canCollideWith(other) of `me`, from what it reads of the other:
// whether it can be collided with and is pushable.
fn can_collide_with_facts(me: &Entity, collided_with: bool, pushable: bool) -> bool {
    match me.kind {
        Kind::Minecart(_) | Kind::Boat(_) => collided_with || pushable,
        _ => collided_with,
    }
}

// The boxes of the entities `me` collides with in a box
// (CollisionUtil.getEntityHardCollisions): an entity that is hard colliding
// looks among all entities, another among the hard colliding ones. The
// entities that collide are those it can collide with; none is a spectator.
// `me` need not be the entity being ticked (an orb made while TNT explodes
// asks whether it is free): that one, out of the level's map, is met by what
// the level knows of it.
pub fn hard_collisions(l: &Level, me: &Entity, aabb: &Aabb) -> Vec<Aabb> {
    let aabb = aabb.inflate(-1.0E-7, -1.0E-7, -1.0E-7);
    let me_hard = is_hard_colliding(&me.kind);
    let mut out = Vec::new();
    for id in super::ids_around(l, &aabb) {
        if id == me.id {
            continue;
        }
        if let Some(t) = l.entities.ticking {
            if t.id == id {
                if t.bb.intersects(&aabb) && (me_hard || t.hard) && can_collide_with_facts(me, t.collided_with, t.pushable) {
                    out.push(t.bb);
                }
                continue;
            }
        }
        let other = l.entities.get(Some(me), id);
        if other.bb.intersects(&aabb) && (me_hard || is_hard_colliding(&other.kind)) {
            if let Kind::Golem(_) = other.kind {
                super::golem::unported(other, "is found by a search of the level's entities");
            }
            if can_collide_with(me, other) {
                out.push(other.bb);
            }
        }
    }
    out
}

// Level.noCollision(entity, box) for an entity in the level (Paper's: block
// collisions, then entity ones): nothing collides with the box.
pub fn no_collision(l: &Level, me: &Entity, aabb: &Aabb) -> bool {
    crate::collision::no_collision(l, aabb, context(l, me)) && hard_collisions(l, me, aabb).is_empty()
}
