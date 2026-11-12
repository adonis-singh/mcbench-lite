// CopperChestBlock and WeatheringCopperChestBlock (which only adds random
// ticks) on ChestBlock's code (chest.rs): a copper chest connects to any
// copper chest, and joined halves take the block of the neighbour half.
// Whether the block entity is kept when the block changes to another copper
// chest is blocks::keeps_block_entity.

use crate::blocks::chest::{self, ChestType};
use crate::blocks::Behaviour;
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), ..chest::BEHAVIOUR };

// CopperChestBlock.chestCanConnectTo.
fn chest_can_connect_to(l: &Level, state: State) -> bool {
    l.reg.in_tag(state, "minecraft:copper_chests") && l.reg.has(state, chest::chest_type_property(l))
}

// Block.withPropertiesOf: the block's default state with the values of the
// properties it shares with `state`.
fn with_properties_of(l: &Level, block: BlockId, state: State) -> State {
    let mut result = l.reg.default_state(block);
    let source = &l.reg.blocks[l.block(state) as usize];
    for &(property, _) in &source.properties {
        if l.reg.has(result, property) {
            result = l.reg.with(result, property, l.reg.get(state, property));
        }
    }
    result
}

// A copper chest's weather state (WeatheringCopper.WeatherState's ordinal)
// and whether it is waxed (CopperChestBlock.isWaxed: the waxed ones are
// CopperChestBlock itself, WeatheringCopperChestBlock is not), from its key.
fn weather_and_waxed(l: &Level, block: BlockId) -> (u8, bool) {
    let key = l.reg.blocks[block as usize].key.as_str();
    let waxed = key.starts_with("minecraft:waxed_");
    let rest = key.trim_start_matches("minecraft:").trim_start_matches("waxed_");
    let weather = match rest {
        "copper_chest" => 0,
        "exposed_copper_chest" => 1,
        "weathered_copper_chest" => 2,
        "oxidized_copper_chest" => 3,
        other => panic!("not a copper chest: {other}"),
    };
    assert_eq!(waxed, l.reg.blocks[block as usize].class == "CopperChestBlock", "a copper chest's class");
    (weather, waxed)
}

// CopperChestBlock.unwaxBlock: a waxed chest's state as the chest without
// wax (HoneycombItem.WAX_OFF_BY_BLOCK), an unwaxed one's as it is.
fn unwax(l: &Level, state: State) -> State {
    let (_, waxed) = weather_and_waxed(l, l.block(state));
    if !waxed {
        return state;
    }
    let key = l.reg.blocks[l.block(state) as usize].key.replacen("minecraft:waxed_", "minecraft:", 1);
    with_properties_of(l, l.reg.block_key(&key), state)
}

// CopperChestBlock.getFromCopperBlock(copperBlock, direction, level, pos): the
// chest of the copper block's weather (COPPER_TO_COPPER_CHEST_MAPPING, waxed
// or not), facing the way given, joined to a single copper chest beside it
// that faces the same way (ChestBlock.getChestType), and as the least
// oxidized of the two halves when joined
// (getLeastOxidizedChestOfConnectedBlocks).
pub fn from_copper_block(l: &mut Level, copper: State, direction: Dir, pos: Pos) -> State {
    let key = l.reg.blocks[l.block(copper) as usize].key.as_str();
    let chest = match key.trim_start_matches("minecraft:").trim_start_matches("waxed_") {
        "copper_block" => "minecraft:copper_chest",
        "exposed_copper" => "minecraft:exposed_copper_chest",
        "weathered_copper" => "minecraft:weathered_copper_chest",
        "oxidized_copper" => "minecraft:oxidized_copper_chest",
        other => panic!("not a copper block: {other}"),
    };
    let block = l.reg.block_key(chest);
    // ChestBlock.candidatePartnerFacing.
    fn partner(l: &mut Level, pos: Pos, side: Dir) -> Option<Dir> {
        let neighbor = l.get(pos.relative(side));
        (chest_can_connect_to(l, neighbor) && chest::chest_type(l, neighbor) == ChestType::Single).then(|| l.reg.get_dir(neighbor, crate::blocks::diode::facing(l)))
    }
    let chest_type = if partner(l, pos, direction.clockwise()) == Some(direction) {
        "left"
    } else if partner(l, pos, direction.counter_clockwise()) == Some(direction) {
        "right"
    } else {
        "single"
    };
    let state = l.reg.default_state(block);
    let state = l.reg.with_dir(state, crate::blocks::diode::facing(l), direction);
    let state = l.reg.with_name(state, chest::chest_type_property(l), chest_type);
    // getLeastOxidizedChestOfConnectedBlocks.
    if chest::chest_type(l, state) == ChestType::Single {
        return state;
    }
    let neighbor = l.get(pos.relative(chest::connected_direction(l, state)));
    if !matches!(l.reg.blocks[l.block(neighbor) as usize].class.as_str(), "CopperChestBlock" | "WeatheringCopperChestBlock") {
        return state;
    }
    let ((weather, waxed), (neighbor_weather, neighbor_waxed)) = (weather_and_waxed(l, l.block(state)), weather_and_waxed(l, l.block(neighbor)));
    let (mut first, mut second) = (state, neighbor);
    if waxed != neighbor_waxed {
        first = unwax(l, state);
        second = unwax(l, neighbor);
    }
    let chosen = if weather <= neighbor_weather { l.block(first) } else { l.block(second) };
    with_properties_of(l, chosen, first)
}

// CopperChestBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    let can_connect = chest_can_connect_to(l, neighbor);
    let shaped = chest::update_shape_connecting(l, state, pos, direction, neighbor, can_connect);
    if can_connect && chest::chest_type(l, shaped) != ChestType::Single && chest::connected_direction(l, shaped) == direction {
        return with_properties_of(l, l.block(neighbor), shaped);
    }
    shaped
}
