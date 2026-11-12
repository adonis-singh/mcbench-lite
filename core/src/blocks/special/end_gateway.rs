// EndGatewayBlock and TheEndGatewayBlockEntity: it ages, and every 2400
// ticks it starts a cooldown (a block event tells the clients). An entity
// inside it, while it is not cooling down, is in the portal and starts the
// cooldown; it has no exit portal and this level is not the End, so the portal
// leads nowhere.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::codec::{self, Input, R};
use crate::nbt::{Compound, Tag};
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    entity_inside: Some(entity_inside),
    all_entities: true,
    ..NONE
};

// EndGatewayBlock.entityInside: EntityInsideBlockEvent and EntityPortalEnterEvent
// have no listener. An entity that can use a portal, at a gateway that is not
// cooling down, is inside the portal, and the gateway's cooldown starts.
fn entity_inside(l: &mut Level, state: State, pos: Pos, entity: &mut crate::entity::Entity, _applier: &mut crate::entity::inside::Applier, _past_edges: bool) {
    if !crate::entity::is_alive(entity) {
        return;
    }
    // level.getBlockEntity(pos) makes the block entity if it is missing.
    let Some(BlockEntity::Special(Special::EndGateway(gateway))) = l.block_entity(pos) else { return };
    if gateway.teleport_cooldown > 0 {
        return;
    }
    crate::entity::motion::set_as_inside_portal(entity, l.block(state), pos);
    trigger_cooldown(l, pos, state);
}

// TheEndGatewayBlockEntity.triggerCooldown: the cooldown, and a block event for
// the clients.
fn trigger_cooldown(l: &mut Level, pos: Pos, state: State) {
    let Some(BlockEntity::Special(Special::EndGateway(gateway))) = l.block_entities.get_mut(&pos) else { panic!("no end gateway at {pos}") };
    gateway.teleport_cooldown = 40;
    // (EndGatewayBlock.entityInside passes the position Entity.checkInsideBlocks
    // walks with, a BlockPos.MutableBlockPos, which the queued event keeps.)
    l.block_event_at_cursor(pos, l.block(state), 1, 0);
    super::set_changed(l, pos);
}

// TheEndGatewayBlockEntity: its age and teleport cooldown. It has no exit
// portal.
#[derive(Clone, Debug)]
pub struct Gateway {
    age: i64,
    teleport_cooldown: i32,
    exit_portal: Option<[i32; 3]>,
    exact_teleport: bool,
}

impl Gateway {
    pub fn saved(&self) -> (&'static str, String) {
        let mut out = Compound::new();
        out.put("Age", Tag::Long(self.age));
        if let Some(exit) = self.exit_portal {
            out.put("exit_portal", Tag::IntArray(exit.to_vec()));
        }
        if self.exact_teleport {
            out.put_boolean("ExactTeleport", true);
        }
        out.put("components", Tag::Compound(Compound::new()));
        ("minecraft:end_gateway", out.to_string())
    }

    // TheEndGatewayBlockEntity.loadAdditional: the exit portal is kept where it
    // is in Level.isInSpawnableBounds.
    pub fn load(&mut self, input: &Input) -> R<()> {
        self.age = input.long_or("Age", 0)?;
        self.exit_portal = input
            .read("exit_portal", |tag| {
                let ints = codec::items(tag)?.iter().map(codec::int).collect::<R<Vec<i32>>>()?;
                match <[i32; 3]>::try_from(ints) {
                    Ok(pos) => Ok(pos),
                    Err(_) => codec::fail("Input is not a list of 3 ints"),
                }
            })?
            .filter(|&[x, y, z]| !(y < -20_000_000 || y >= 20_000_000) && x >= -30_000_000 && z >= -30_000_000 && x < 30_000_000 && z < 30_000_000);
        self.exact_teleport = input.bool_or("ExactTeleport", false)?;
        Ok(())
    }

    // The age and teleport cooldown, as the engine trace writes them.
    pub fn state_text(&self) -> String {
        format!("{} {}", self.age, self.teleport_cooldown)
    }

    // TheEndGatewayBlockEntity.triggerEvent.
    pub fn trigger_event(&mut self, id: i32) -> bool {
        if id == 1 {
            self.teleport_cooldown = 40;
            true
        } else {
            false
        }
    }
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::EndGateway(Gateway { age: 0, teleport_cooldown: 0, exit_portal: None, exact_teleport: false }))
}

// EndGatewayBlock.getTicker on the server: TheEndGatewayBlockEntity.portalTick.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(portal_tick)
}

fn portal_tick(l: &mut Level, pos: Pos, state: State) {
    let Some(BlockEntity::Special(Special::EndGateway(gateway))) = l.block_entities.get_mut(&pos) else { panic!("no end gateway at {pos}") };
    let is_spawning = gateway.age < 200;
    let is_cooling_down = gateway.teleport_cooldown > 0;
    gateway.age += 1;
    let mut trigger = false;
    if is_cooling_down {
        gateway.teleport_cooldown -= 1;
    } else if gateway.age % 2400 == 0 {
        // triggerCooldown: the clients are told by a block event.
        gateway.teleport_cooldown = 40;
        trigger = true;
    }
    let changed = is_spawning != (gateway.age < 200) || is_cooling_down != (gateway.teleport_cooldown > 0);
    if trigger {
        l.block_event(pos, l.block(state), 1, 0);
        super::set_changed(l, pos);
    }
    if changed {
        super::set_changed(l, pos);
    }
}
