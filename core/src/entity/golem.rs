// The golems a carved pumpkin makes (CarvedPumpkinBlock.trySpawnGolem): a snow
// golem, an iron golem or a copper golem, made (EntityType.create: what
// Entity's constructor draws, and the copper golem's first transport cooldown
// from the shared random), put on its pattern (snapTo) and added to the level,
// which is what blocks/shapes/carved_pumpkin.rs does around them.
//
// What a golem does from then on is its AI, not ported: a snow golem's and an
// iron golem's goals (GoalSelector, with LookAtPlayerGoal's and
// RandomLookAroundGoal's draws on its first tick, pathfinding, the snow golem's
// trail and snowballs, the iron golem's targets), a copper golem's brain
// (sensors, TransportItemsBetweenContainers between copper chests and chests,
// RandomStroll, the door and look behaviours, PathNavigation and
// WalkNodeEvaluator, MoveControl and LookControl) and its oxidation into a
// statue, with everything a LivingEntity is (attributes, health and damage,
// knockback, pushing and being pushed, death and its drops). A golem refuses
// when it would first tick, and so does any search of the level's entities that
// finds one (a pressure plate, a tripwire, an explosion, a piston, an entity
// that collides with it). (A copper golem's sensors also tick on a phase drawn
// from Sensor.RANDOM, a random Paper seeds from the clock, which a seeded
// level does not fix: two runs of Paper itself need not agree on it.)

use super::{Entity, Kind};
use crate::level::{refuse, Level};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Golem {
    Snow,
    Iron,
    Copper,
}

impl Golem {
    pub fn key(self) -> &'static str {
        match self {
            Golem::Snow => "minecraft:snow_golem",
            Golem::Iron => "minecraft:iron_golem",
            Golem::Copper => "minecraft:copper_golem",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Golem::Snow => "a snow golem",
            Golem::Iron => "an iron golem",
            Golem::Copper => "a copper golem",
        }
    }
}

pub struct GolemData {
    pub golem: Golem,
    // IronGolem.setPlayerCreated.
    pub player_created: bool,
    // CopperGolem's weather state (WeatheringCopper.WeatherState's ordinal)
    // and the transport cooldown its brain starts with.
    pub weather: u8,
    pub transport_cooldown: i32,
}

// EntityType.create(level, TRIGGERED) for a golem: Entity's constructor (the
// id and the UUID from the shared random); LivingEntity's draws its rotation
// from Math.random, which snapTo replaces; a copper golem's draws its first
// transport cooldown (getRandom().nextInt(60, 100)).
pub fn new_golem(l: &mut Level, golem: Golem) -> Box<Entity> {
    let t = l.reg.entity_types[golem.key()].clone();
    let data = GolemData { golem, player_created: false, weather: 0, transport_cooldown: 0 };
    let mut e = Box::new(Entity::new(l, Kind::Golem(data), t.width, t.height, t.eye_height));
    if golem == Golem::Copper {
        let cooldown = 60 + l.shared_random.next_int_bound(40);
        data_mut(&mut e).transport_cooldown = cooldown;
    }
    e
}

pub fn data(e: &Entity) -> &GolemData {
    match &e.kind {
        Kind::Golem(d) => d,
        _ => panic!("not a golem"),
    }
}

pub fn data_mut(e: &mut Entity) -> &mut GolemData {
    match &mut e.kind {
        Kind::Golem(d) => d,
        _ => panic!("not a golem"),
    }
}

// Entity.snapTo(x, y, z, 0, 0).
pub fn snap_to(l: &mut Level, e: &mut Entity, x: f64, y: f64, z: f64) {
    super::motion::set_pos(l, e, x, y, z);
    e.y_rot = 0.0;
    e.x_rot = 0.0;
    e.set_old_pos_and_rot();
}

// A golem's tick, or anything else that would read what its AI or its body
// does: not ported.
#[cold]
pub fn unported(e: &Entity, what: &str) -> ! {
    refuse(format!("{} {what} (its AI is not ported)", data(e).golem.name()))
}
