// NetherPortalBlock. Its updateShape breaks the portal when its frame is
// no longer complete; random ticks (zombified piglin spawns) never run, and
// an entity inside it is in the portal (Entity.setAsInsidePortal), which leads
// nowhere in a level that is the server's only one.

use super::portal_shape;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Axis, Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), entity_inside: Some(entity_inside), all_entities: true, ..NONE };

// NetherPortalBlock.entityInside: EntityInsideBlockEvent and
// EntityPortalEnterEvent have no listener; an entity that can use a portal
// (alive, nothing rides here) is inside it.
fn entity_inside(l: &mut Level, state: State, pos: Pos, entity: &mut crate::entity::Entity, _applier: &mut crate::entity::inside::Applier, _past_edges: bool) {
    if crate::entity::is_alive(entity) {
        crate::entity::motion::set_as_inside_portal(entity, l.block(state), pos);
    }
}

fn axis_property(l: &Level) -> u16 {
    property!(l, "axis", ["x", "z"])
}

// NetherPortalBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    let axis = direction.axis();
    let state_axis = if l.reg.value_name(state, axis_property(l)) == "x" { Axis::X } else { Axis::Z };
    let flag = state_axis != axis && axis != Axis::Y;
    if !flag && l.block(neighbor) != l.block(state) && !portal_shape::find_any_shape(l, pos, state_axis).is_complete() {
        l.known.air
    } else {
        state
    }
}
