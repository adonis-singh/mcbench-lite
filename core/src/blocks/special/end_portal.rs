// EndPortalBlock and TheEndPortalBlockEntity. An entity inside an end portal
// is in the portal (Entity.setAsInsidePortal), which leads nowhere in a level
// that is the server's only one (the End is off).

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    entity_inside: Some(entity_inside),
    all_entities: true,
    ..NONE
};

// EndPortalBlock.entityInside: EntityInsideBlockEvent and EntityPortalEnterEvent
// have no listener; this level is not the End and nothing is a player, so an
// entity that can use a portal is inside it.
fn entity_inside(l: &mut Level, state: State, pos: Pos, entity: &mut crate::entity::Entity, _applier: &mut crate::entity::inside::Applier, _past_edges: bool) {
    if crate::entity::is_alive(entity) {
        crate::entity::motion::set_as_inside_portal(entity, l.block(state), pos);
    }
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::EndPortal)
}
