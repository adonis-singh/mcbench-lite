// RedStoneOreBlock: an entity that steps on it lights it (a player's
// punch or a held item does too, and none is here). The particles it spawns
// draw their random positions from the level's random on the server too.

use crate::blocks::{Behaviour, NONE};
use crate::dir::Pos;
use crate::entity::Entity;
use crate::level::{Level, UPDATE_ALL};
use crate::property;
use crate::registry::{State, SOLID_RENDER};

pub const BEHAVIOUR: Behaviour = Behaviour { step_on: Some(step_on), ..NONE };

fn lit(l: &Level) -> u16 {
    property!(l, "lit", ["true", "false"])
}

// RedStoneOreBlock.stepOn: an item entity does not step carefully, and
// EntityInteractEvent has no listener.
fn step_on(l: &mut Level, pos: Pos, _state: State, _entity: &Entity) {
    let state = l.get(pos);
    interact(l, state, pos);
}

// RedStoneOreBlock.interact. EntityChangeBlockEvent has no listener.
fn interact(l: &mut Level, state: State, pos: Pos) {
    spawn_particles(l, pos);
    if !l.reg.get_bool(state, lit(l)) {
        l.set(pos, l.reg.with_bool(state, lit(l), true), UPDATE_ALL);
    }
}

// RedStoneOreBlock.spawnParticles: no particle is made on the server, but
// the two coordinates not on the face's axis are drawn from the level's
// random for every face without a solid block against it.
fn spawn_particles(l: &mut Level, pos: Pos) {
    for dir in crate::dir::ALL {
        let state = l.get(pos.relative(dir));
        if l.reg.flags(state) & SOLID_RENDER == 0 {
            l.random.next_float();
            l.random.next_float();
        }
    }
}
