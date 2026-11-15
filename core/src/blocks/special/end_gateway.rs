// EndGatewayBlock and TheEndGatewayBlockEntity: it ages, and every 2400
// ticks it starts a cooldown (a block event tells the clients). An entity
// inside it, while it is not cooling down, is in the portal and starts the
// cooldown. The portal leads to its exit portal (portal_position), or, with
// none, nowhere: this level is not the End, where a gateway would make one.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::codec::{self, Input, R};
use crate::nbt::{Compound, Tag};
use crate::dir::Pos;
use crate::level::Level;
use crate::phys::Vec3;
use crate::registry::State;
use crate::world::{HEIGHT, MIN_Y};

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

// EndGatewayBlock.getPortalDestination: TheEndGatewayBlockEntity.getPortalPosition
// of the gateway at `pos`, outside the End: at the bottom center of its exit
// portal (ExactTeleport) or of the place findExitPosition finds by it, or
// none for a gateway without one (or a position without a gateway).
pub fn portal_position(l: &mut Level, pos: Pos) -> Option<Vec3> {
    let Some(BlockEntity::Special(Special::EndGateway(gateway))) = l.block_entity(pos) else { return None };
    let [x, y, z] = gateway.exit_portal?;
    let exact = gateway.exact_teleport;
    let exit = Pos::new(x, y, z);
    // findExitPosition: findTallestBlock(level, exit + (0, 2, 0), 5, false).above().
    let at = if exact { exit } else { find_tallest_block(l, Pos::new(x, y + 2, z), 5, false).above() };
    Some(Vec3::new(at.x() as f64 + 0.5, at.y() as f64, at.z() as f64 + 0.5))
}

// TheEndGatewayBlockEntity.findTallestBlock: of the columns within `radius`
// of `pos` (its own only with bedrock allowed), the highest block whose
// collision shape is a full block (bedrock only if allowed), each column
// read from the top of the level down to the best found so far; `pos` if
// there is none. A column in a chunk the build does not hold is air, as the
// verifier's void generator makes it.
fn find_tallest_block(l: &mut Level, pos: Pos, radius: i32, allow_bedrock: bool) -> Pos {
    let bedrock = l.reg.block_key("minecraft:bedrock");
    let mut found: Option<Pos> = None;
    for i in -radius..=radius {
        for i1 in -radius..=radius {
            if i == 0 && i1 == 0 && !allow_bedrock {
                continue;
            }
            let bottom = found.map_or(MIN_Y, |f| f.y());
            let mut y = MIN_Y + HEIGHT - 1;
            while y > bottom {
                let at = Pos::new(pos.x() + i, y, pos.z() + i1);
                let state = l.get(at);
                if crate::shape_ops::is_collision_shape_full_block(l, state, at) && (allow_bedrock || l.block(state) != bedrock) {
                    found = Some(at);
                    break;
                }
                y -= 1;
            }
        }
    }
    found.unwrap_or(pos)
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

// TheEndGatewayBlockEntity: its age, teleport cooldown, exit portal (in a
// save's coordinates) and whether the exit is exact.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{Refusal, UPDATE_CLIENTS};
    use crate::loot::ItemStack;

    thread_local! {
        static BOOT: (&'static crate::registry::Registry, &'static crate::blocks::Blocks) = crate::boot();
    }

    // A verifier level with the chunks around the origin ticketed, stone at
    // y 63, a gateway at (1, 64, 1) with the exit `exit` (exact or not) and a
    // diamond falling into it.
    fn level_with_gateway(exit: [i32; 3], exact: bool) -> (Level, crate::entity::EntityId) {
        let (reg, blocks) = BOOT.with(|boot| *boot);
        let mut l = Level::new(reg, blocks, 0, false);
        for x in -1..=1 {
            for z in -1..=1 {
                l.ticket(x, z);
            }
        }
        for x in -8..24 {
            for z in -8..24 {
                l.set(Pos::new(x, 63, z), reg.parse_state("minecraft:stone").0, UPDATE_CLIENTS);
            }
        }
        let at = Pos::new(1, 64, 1);
        l.set(at, reg.parse_state("minecraft:end_gateway").0, UPDATE_CLIENTS);
        let Some(BlockEntity::Special(Special::EndGateway(gateway))) = l.block_entity(at) else { panic!("no gateway") };
        gateway.exit_portal = Some(exit);
        gateway.exact_teleport = exact;
        crate::entity::item::spawn_with_default_delay(&mut l, 1.5, 65.0, 1.5, ItemStack::new(reg.item_by_key["minecraft:diamond"], 1));
        let id = l.entities.counter;
        (l, id)
    }

    fn pos_of(l: &Level, id: crate::entity::EntityId) -> (f64, f64, f64) {
        let e = &l.entities.all[&id];
        (e.pos.x, e.pos.y, e.pos.z)
    }

    // An entity in a gateway with an exact exit goes to the exit's bottom
    // center on the tick it comes in, keeping its motion, and the gateway
    // cools down for 40 ticks.
    #[test]
    fn an_item_in_a_gateway_goes_to_its_exact_exit() {
        let (mut l, id) = level_with_gateway([6, 64, 1], true);
        let mut ticks = 0;
        while pos_of(&l, id).0 < 6.0 {
            l.tick();
            ticks += 1;
            assert!(ticks < 20, "the item never went");
        }
        assert_eq!(pos_of(&l, id), (6.5, 64.0, 1.5));
        assert_eq!(l.entities.all[&id].portal_cooldown, 300);
        // Its own tick, after the entities', has taken one off.
        let Some(BlockEntity::Special(Special::EndGateway(gateway))) = l.block_entity(Pos::new(1, 64, 1)) else { panic!("no gateway") };
        assert_eq!(gateway.teleport_cooldown, 39);
    }

    // Not exact: findExitPosition puts it above the tallest full block within
    // five columns of the exit (not the exit's own column, not bedrock).
    #[test]
    fn an_item_in_a_gateway_goes_above_the_tallest_block_by_a_loose_exit() {
        let (mut l, id) = level_with_gateway([10, 64, 1], false);
        let stone = l.reg.parse_state("minecraft:stone").0;
        let bedrock = l.reg.parse_state("minecraft:bedrock").0;
        for y in 64..70 {
            l.set(Pos::new(10, y, 1), stone, UPDATE_CLIENTS);
            l.set(Pos::new(12, y, 1), bedrock, UPDATE_CLIENTS);
        }
        for y in 64..67 {
            l.set(Pos::new(8, y, 3), stone, UPDATE_CLIENTS);
        }
        let mut ticks = 0;
        while pos_of(&l, id).0 < 6.0 {
            l.tick();
            ticks += 1;
            assert!(ticks < 20, "the item never went");
        }
        assert_eq!(pos_of(&l, id), (8.5, 67.0, 3.5));
    }

    // An exit whose portal chunk ticket would make chunks tick that the
    // level's tickets do not is refused.
    #[test]
    fn an_exit_beyond_the_levels_tickets_is_refused() {
        let (mut l, _) = level_with_gateway([40, 64, 1], true);
        let ticked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for _ in 0..20 {
                l.tick();
            }
        }));
        let refusal = ticked.expect_err("the teleport is refused");
        assert!(refusal.downcast_ref::<Refusal>().expect("a refusal").0.contains("portal chunk ticket"));
    }
}
