// VaultBlock and VaultBlockEntity (VaultServerData, VaultSharedData,
// VaultState), with the default configuration, in a level without players:
// no player is ever connected to a vault, so it never activates or ejects.
// An active vault shows an item rolled from a loot table every 20 game
// ticks, which this port does not model.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::Pos;
use crate::level::{refuse, Level, UPDATE_ALL};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    ..NONE
};

// VaultBlockEntity: when the server data lets the state update again.
#[derive(Clone, Debug, Default)]
pub struct Vault {
    state_updating_resumes_at: i64,
}

impl Vault {
    pub fn saved(&self) -> (&'static str, String) {
        let resumes = if self.state_updating_resumes_at != 0 { format!("state_updating_resumes_at:{}L", self.state_updating_resumes_at) } else { String::new() };
        ("minecraft:vault", format!("{{components:{{}},config:{{key_item:{{count:1,id:\"minecraft:trial_key\"}}}},server_data:{{{resumes}}},shared_data:{{}}}}"))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Inactive,
    Active,
    Unlocking,
    Ejecting,
}

fn state_property(l: &Level) -> u16 {
    property!(l, "vault_state", ["inactive", "active", "unlocking", "ejecting"])
}

fn phase(l: &Level, state: State) -> Phase {
    match l.reg.value_name(state, state_property(l)) {
        "inactive" => Phase::Inactive,
        "active" => Phase::Active,
        "unlocking" => Phase::Unlocking,
        "ejecting" => Phase::Ejecting,
        other => panic!("no vault state {other}"),
    }
}

fn with_phase(l: &Level, state: State, phase: Phase) -> State {
    l.reg.with_name(
        state,
        state_property(l),
        match phase {
            Phase::Inactive => "inactive",
            Phase::Active => "active",
            Phase::Unlocking => "unlocking",
            Phase::Ejecting => "ejecting",
        },
    )
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::Vault(Vault::default()))
}

// VaultBlock.getTicker on the server: VaultBlockEntity.Server.tick.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(tick)
}

fn vault(l: &mut Level, pos: Pos) -> &mut Vault {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Special(Special::Vault(vault))) => vault,
        _ => panic!("no vault at {pos}"),
    }
}

// VaultBlockEntity.Server.tick. No player is within any range, so the
// connected players stay empty (updateConnectedPlayersWithinRange changes
// nothing), nothing is left to eject, and the shown item is empty.
fn tick(l: &mut Level, pos: Pos, state: State) {
    let current = phase(l, state);
    if l.game_time % 20 == 0 && current == Phase::Active {
        refuse(format!("vault at {pos} rolls the item it shows from its loot table"));
    }
    if l.game_time >= vault(l, pos).state_updating_resumes_at {
        let next = match current {
            // updateStateForConnectedPlayers: nobody is connected.
            Phase::Inactive | Phase::Active => {
                vault(l, pos).state_updating_resumes_at = l.game_time + 20;
                Phase::Inactive
            }
            Phase::Unlocking => {
                vault(l, pos).state_updating_resumes_at = l.game_time + 20;
                Phase::Ejecting
            }
            // Nothing to eject: the ejection is finished.
            Phase::Ejecting => {
                vault(l, pos).state_updating_resumes_at = l.game_time + 20;
                Phase::Inactive
            }
        };
        if next != current {
            // setVaultState: the event has no listener; the transition's
            // sounds and level events reach no one, and the shown item is
            // already empty.
            l.set(pos, with_phase(l, state, next), UPDATE_ALL);
        }
        // The server data was changed (the pause): setChanged.
        super::set_changed(l, pos);
    }
}
