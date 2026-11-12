// The special blocks: sculk, command and structure blocks, spawners, vaults,
// beacons, conduits, portals, fire, pressure plates and the rest that are
// neither redstone components nor decoration. A module per Java class (the
// sculk sensor module also holds the calibrated sensor's difference); this
// module has the block entities they put in the level.
//
// A level without players has no one to open, step on, power or wake any of
// them: what each class does here is the part of its code that reads only
// the world.

use super::BlockEntity;
use crate::dir::Pos;
use crate::game_event::{Context, GameEvent};
use crate::level::Level;
use crate::phys::Vec3;
use crate::registry::State;

pub mod amethyst_cluster;
pub mod beacon;
pub mod beehive;
pub mod campfire;
pub mod command_block;
pub mod conduit;
pub mod creaking_heart;
pub mod decorated_pot;
pub mod enchanting_table;
pub mod end_gateway;
pub mod end_portal;
pub mod end_portal_frame;
pub mod fire;
pub mod frosted_ice;
pub mod grindstone;
pub mod jigsaw;
pub mod lightning_rod;
pub mod nether_portal;
pub mod portal_shape;
pub mod powder_snow;
pub mod pressure_plate;
pub mod redstone_ore;
pub mod respawn_anchor;
pub mod sculk_catalyst;
pub mod sculk_sensor;
pub mod sculk_shrieker;
pub mod spawner;
pub mod structure_block;
pub mod suspicious;
pub mod test_block;
pub mod test_instance_block;
pub mod trial_spawner;
pub mod vault;
pub mod vibration;
pub mod waterlogged;
pub mod weighted_pressure_plate;

// Whose vibrations a listener hears (VibrationSystem.User).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UserKind {
    Sensor,
    Calibrated,
    Shrieker,
}

#[derive(Clone, Debug)]
pub enum Special {
    // SculkSensorBlockEntity and CalibratedSculkSensorBlockEntity.
    SculkSensor(sculk_sensor::Sensor),
    // SculkCatalystBlockEntity: no sculk spreader cursor exists (a cursor
    // comes from a living entity's death).
    SculkCatalyst,
    SculkShrieker(sculk_shrieker::Shrieker),
    CommandBlock(command_block::CommandBlock),
    // SpawnerBlockEntity: nothing changes without a player near.
    Spawner,
    TrialSpawner(trial_spawner::TrialSpawner),
    Vault(vault::Vault),
    Beacon(beacon::Beacon),
    Beehive,
    Campfire,
    Conduit(conduit::Conduit),
    // EnchantingTableBlockEntity: its custom name (CustomName, as the tag its codec writes).
    EnchantingTable { name: Option<crate::nbt::Tag> },
    EndPortal,
    EndGateway(end_gateway::Gateway),
    CreakingHeart(creaking_heart::CreakingHeart),
    // BrushableBlockEntity: the item it holds, as the tag ItemStack.CODEC writes (none brushes it).
    BrushableBlock { item: Option<crate::nbt::Compound> },
    StructureBlock(structure_block::StructureBlock),
    Jigsaw(jigsaw::Jigsaw),
    TestBlock(test_block::TestBlock),
    TestInstanceBlock,
}

impl Special {
    // The class of the one kind of block the entity's type is valid for
    // (BlockEntityType.isValid).
    fn block_class(&self) -> &'static str {
        match self {
            Special::SculkSensor(sensor) => {
                if sensor.calibrated {
                    "CalibratedSculkSensorBlock"
                } else {
                    "SculkSensorBlock"
                }
            }
            Special::SculkCatalyst => "SculkCatalystBlock",
            Special::SculkShrieker(_) => "SculkShriekerBlock",
            Special::CommandBlock(_) => "CommandBlock",
            Special::Spawner => "SpawnerBlock",
            Special::TrialSpawner(_) => "TrialSpawnerBlock",
            Special::Vault(_) => "VaultBlock",
            Special::Beacon(_) => "BeaconBlock",
            Special::Beehive => "BeehiveBlock",
            Special::Campfire => "CampfireBlock",
            Special::Conduit(_) => "ConduitBlock",
            Special::EnchantingTable { .. } => "EnchantingTableBlock",
            Special::EndPortal => "EndPortalBlock",
            Special::EndGateway(_) => "EndGatewayBlock",
            Special::CreakingHeart(_) => "CreakingHeartBlock",
            Special::BrushableBlock { .. } => "BrushableBlock",
            Special::StructureBlock(_) => "StructureBlock",
            Special::Jigsaw(_) => "JigsawBlock",
            Special::TestBlock(_) => "TestBlock",
            Special::TestInstanceBlock => "TestInstanceBlock",
        }
    }
}

// BlockEntityType.isValid.
pub fn valid(l: &Level, entity: &Special, state: State) -> bool {
    l.reg.blocks[l.block(state) as usize].class == entity.block_class()
}

// The entity's type key and saved data (see BlockEntity::saved).
pub fn saved(l: &crate::registry::Registry, entity: &Special) -> (&'static str, String) {
    match entity {
        Special::SculkSensor(sensor) => sensor.saved(l),
        Special::SculkCatalyst => ("minecraft:sculk_catalyst", "{components:{},cursors:[]}".into()),
        Special::SculkShrieker(shrieker) => shrieker.saved(l),
        Special::CommandBlock(command) => command.saved(),
        Special::Spawner => (
            "minecraft:mob_spawner",
            "{Delay:20s,MaxNearbyEntities:6s,MaxSpawnDelay:800s,MinSpawnDelay:200s,RequiredPlayerRange:16s,SpawnCount:4s,SpawnPotentials:[],SpawnRange:4s,components:{}}".into(),
        ),
        Special::TrialSpawner(spawner) => spawner.saved(),
        Special::Vault(vault) => vault.saved(),
        Special::Beacon(beacon) => beacon.saved(),
        Special::Beehive => ("minecraft:beehive", "{Bukkit.MaxEntities:3,bees:[],components:{}}".into()),
        Special::Campfire => ("minecraft:campfire", "{CookingTimes:[I;0,0,0,0],CookingTotalTimes:[I;0,0,0,0],Items:[],Paper.StopCooking:[B;0B,0B,0B,0B],components:{}}".into()),
        Special::Conduit(_) => ("minecraft:conduit", "{components:{}}".into()),
        Special::EnchantingTable { name } => {
            let mut out = crate::nbt::Compound::new();
            if let Some(name) = name {
                out.put("CustomName", name.clone());
            }
            out.put("components", crate::nbt::Tag::Compound(crate::nbt::Compound::new()));
            ("minecraft:enchanting_table", out.to_string())
        }
        Special::EndPortal => ("minecraft:end_portal", "{components:{}}".into()),
        Special::EndGateway(gateway) => gateway.saved(),
        Special::CreakingHeart(_) => ("minecraft:creaking_heart", "{components:{}}".into()),
        Special::BrushableBlock { item } => {
            let mut out = crate::nbt::Compound::new();
            if let Some(item) = item {
                out.put("item", crate::nbt::Tag::Compound(item.clone()));
            }
            out.put("components", crate::nbt::Tag::Compound(crate::nbt::Compound::new()));
            ("minecraft:brushable_block", out.to_string())
        }
        Special::StructureBlock(structure) => structure.saved(),
        Special::Jigsaw(jigsaw) => jigsaw.saved(),
        Special::TestBlock(test) => test.saved(),
        Special::TestInstanceBlock => ("minecraft:test_instance_block", "{components:{},data:{ignore_entities:0b,rotation:\"none\",size:[I;0,0,0],status:\"cleared\"}}".into()),
    }
}

// BlockEntity.preRemoveSideEffects. A block entity that is a Container drops
// its contents (Containers.dropContents): the campfire's four slots are
// empty, but dropItemStack draws the item's three position doubles from the
// level's random before it looks at the stack. The sculk shrieker would
// answer a warning, and the creaking heart would tear down its creaking:
// neither can here.
pub fn pre_remove_side_effects(l: &mut Level, entity: &Special) {
    let slots = match entity {
        Special::Campfire => 4,
        _ => 0,
    };
    for _ in 0..slots * 3 {
        l.random.next_double();
    }
}

// BaseEntityBlock.triggerEvent: Block's returns false, then the block
// entity (Level.getBlockEntity makes it if missing) answers.
pub fn trigger_event(l: &mut Level, _state: State, pos: Pos, id: i32, param: i32) -> bool {
    match l.block_entity(pos) {
        Some(BlockEntity::Special(entity)) => match entity {
            // TheEndGatewayBlockEntity.triggerEvent.
            Special::EndGateway(gateway) => gateway.trigger_event(id),
            // SpawnerBlockEntity.triggerEvent: BaseSpawner.onEventTriggered
            // is true for the spawn event, which only the client acts on.
            Special::Spawner => id == 1,
            _ => false,
        },
        Some(_) | None => false,
    }
}

// Whether the block entity listens to game events (EntityBlock.getListener).
pub fn is_listener(entity: &BlockEntity) -> bool {
    matches!(entity, BlockEntity::Special(Special::SculkSensor(_) | Special::SculkCatalyst | Special::SculkShrieker(_)))
}

// The range a listener at `pos` hears events in.
pub fn listener_radius(l: &Level, pos: Pos) -> i32 {
    match l.block_entities.get(&pos) {
        Some(BlockEntity::Special(Special::SculkCatalyst)) => 8,
        Some(BlockEntity::Special(Special::SculkSensor(_) | Special::SculkShrieker(_))) => vibration::listener_radius(l, pos),
        other => panic!("no game event listener at {pos}: {other:?}"),
    }
}

// GameEventListener.handleGameEvent of the listener at `pos`.
pub fn handle_game_event(l: &mut Level, listener: Pos, event: GameEvent, context: Context, pos: Vec3) {
    match l.block_entities.get(&listener) {
        // SculkCatalystBlockEntity.CatalystListener: a living entity's death
        // is what it hears.
        Some(BlockEntity::Special(Special::SculkCatalyst)) => sculk_catalyst::handle_game_event(l, listener, event, context),
        Some(BlockEntity::Special(Special::SculkSensor(_) | Special::SculkShrieker(_))) => {
            vibration::handle_game_event(l, listener, event, context, pos);
        }
        other => panic!("no game event listener at {listener}: {other:?}"),
    }
}

// BlockEntity.setChanged: the entity's block is not air, so the comparators
// that read a container beside it are told (Level.updateNeighbourForOutputSignal).
pub fn set_changed(l: &mut Level, pos: Pos) {
    let state = l.get(pos);
    if !l.is_air(state) {
        l.update_neighbour_for_output_signal(pos, l.block(state));
    }
}

// CraftBlockEntityState's snapshot of a block entity: the data a listener
// block entity saves (its vibration data), which place() loads back into the
// entity that is there. The other entities here do not change between the
// snapshot and the place.
pub fn snapshot_listener(l: &Level, pos: Pos) -> Option<Special> {
    match l.block_entities.get(&pos) {
        Some(BlockEntity::Special(special @ (Special::SculkSensor(_) | Special::SculkShrieker(_)))) => Some(special.clone()),
        _ => None,
    }
}

// CraftBlockEntityState.applyTo of that snapshot.
pub fn apply_snapshot(l: &mut Level, pos: Pos, snapshot: Option<Special>) {
    let Some(snapshot) = snapshot else { return };
    match (l.block_entities.get_mut(&pos), snapshot) {
        (Some(BlockEntity::Special(Special::SculkSensor(entity))), Special::SculkSensor(saved)) => {
            entity.vibration = saved.vibration;
            entity.last_vibration_frequency = saved.last_vibration_frequency;
        }
        (Some(BlockEntity::Special(Special::SculkShrieker(entity))), Special::SculkShrieker(saved)) => {
            entity.vibration = saved.vibration;
            entity.warning_level = saved.warning_level;
        }
        _ => crate::level::refuse(format!("a listener block entity at {pos} replaced while a sponge absorbs")),
    }
}

// A block's ticker for a block entity whose class ticks nothing here.
pub fn no_ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    None
}

// A ticker that does nothing for a block entity whose class ticks, but
// whose tick reads only players or things that are not here.
pub fn idle_ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    fn idle(_l: &mut Level, _pos: Pos, _state: State) {}
    Some(idle)
}
