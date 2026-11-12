// PistonBaseBlock (the piston and the sticky piston), with Paper's piston
// code and its settings at their defaults (unsupported-settings
// allow-permanent-block-break-exploits and allow-piston-duplication false);
// the Bukkit piston events have no listener, so each goes on. Entities
// pushed by the blocks this makes are the moving block entity's
// (moving_piston.rs); the level holds none.

use super::moving_piston::{self, Moving};
use super::piston_resolver::Resolver;
use super::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos, ALL};
use crate::jmap::JavaHashMap;
use crate::level::Level;
use crate::property;
use crate::registry::{BlockId, Push, State};
use crate::world::{HEIGHT, MIN_Y};
use std::sync::OnceLock;

pub const BEHAVIOUR: Behaviour = Behaviour {
    neighbor_changed: Some(neighbor_changed),
    on_place: Some(on_place),
    trigger_event: Some(trigger_event),
    ..NONE
};

// Blocks by key, looked up once.
macro_rules! block {
    ($name:ident, $key:literal) => {
        pub fn $name(l: &Level) -> BlockId {
            static ID: OnceLock<BlockId> = OnceLock::new();
            *ID.get_or_init(|| l.reg.block_key($key))
        }
    };
}

block!(piston, "minecraft:piston");
block!(sticky_piston, "minecraft:sticky_piston");
block!(piston_head, "minecraft:piston_head");
block!(moving_piston_block, "minecraft:moving_piston");
block!(slime_block, "minecraft:slime_block");
block!(honey_block, "minecraft:honey_block");
block!(obsidian, "minecraft:obsidian");
block!(crying_obsidian, "minecraft:crying_obsidian");
block!(respawn_anchor, "minecraft:respawn_anchor");
block!(reinforced_deepslate, "minecraft:reinforced_deepslate");

// DirectionalBlock.FACING.
pub fn facing(l: &Level) -> u16 {
    property!(l, "facing", ["north", "east", "south", "west", "up", "down"])
}

// PistonBaseBlock.EXTENDED.
pub fn extended(l: &Level) -> u16 {
    property!(l, "extended", ["true", "false"])
}

// PistonHeadBlock.TYPE, MovingPistonBlock.TYPE: a PistonType.
pub fn piston_type(l: &Level) -> u16 {
    property!(l, "type", ["normal", "sticky"])
}

// Direction.from3DDataValue.
pub fn from_3d_data_value(index: i32) -> Dir {
    ALL[(index % 6).unsigned_abs() as usize]
}

// BlockStateBase.getPistonPushReaction.
pub fn push_reaction(l: &Level, state: State) -> Push {
    l.reg.states[state as usize].push
}


// PistonBaseBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    check_if_extend(l, pos, state);
}

// PistonBaseBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    if l.block(old) != l.block(state) && l.block_entity(pos).is_none() {
        check_if_extend(l, pos, state);
    }
}

// PistonBaseBlock.checkIfExtend.
fn check_if_extend(l: &mut Level, pos: Pos, state: State) {
    let direction = l.reg.get_dir(state, facing(l));
    let neighbor_signal = get_neighbor_signal(l, pos, direction);
    let extended_value = l.reg.get_bool(state, extended(l));
    if neighbor_signal && !extended_value {
        if Resolver::new(pos, direction, true).resolve(l) {
            l.block_event(pos, l.block(state), 0, direction as i32);
        }
    } else if !neighbor_signal && extended_value {
        let block_pos = pos.relative_n(direction, 2);
        let block_state = l.get(block_pos);
        let mut i = 1;
        if l.block(block_state) == moving_piston_block(l) && l.reg.get_dir(block_state, facing(l)) == direction {
            if let Some(BlockEntity::Piston(moving)) = l.block_entity(block_pos).map(|entity| entity.clone()) {
                if moving.is_extending() && (moving.get_progress() < 0.5 || l.game_time == moving.last_ticked() || l.handling_tick) {
                    i = 2;
                }
            }
        }
        l.block_event(pos, l.block(state), i, direction as i32);
    }
}

// PistonBaseBlock.getNeighborSignal: the piston's neighbours, the piston
// itself from below, and (quasi-connectivity) the neighbours of the block
// above it.
fn get_neighbor_signal(l: &mut Level, pos: Pos, direction: Dir) -> bool {
    for direction1 in ALL {
        if direction1 != direction && l.has_signal(pos.relative(direction1), direction1) {
            return true;
        }
    }
    if l.has_signal(pos, Dir::Down) {
        return true;
    }
    let above = pos.above();
    for direction2 in ALL {
        if direction2 != Dir::Down && l.has_signal(above.relative(direction2), direction2) {
            return true;
        }
    }
    false
}

// PistonBaseBlock.triggerEvent.
fn trigger_event(l: &mut Level, state: State, pos: Pos, id: i32, param: i32) -> bool {
    let sticky = l.block(state) == sticky_piston(l);
    let direction = l.reg.get_dir(state, facing(l));
    let direction_queued_as = from_3d_data_value(param & 7);
    if direction != direction_queued_as {
        return false;
    }
    let extended_state = l.reg.with_bool(state, extended(l), true);
    let neighbor_signal = get_neighbor_signal(l, pos, direction);
    if neighbor_signal && (id == 1 || id == 2) {
        l.set(pos, extended_state, 2);
        return false;
    }
    if !neighbor_signal && id == 0 {
        return false;
    }
    if id == 0 {
        if !move_blocks(l, pos, direction, true, sticky) {
            return false;
        }
        l.set(pos, extended_state, 67);
        // The sound's pitch.
        l.random.next_float();
        let event = crate::game_event!(l, "block_activate");
        l.game_event_in(event, pos, crate::game_event::Context::of_state(extended_state));
    } else if id == 1 || id == 2 {
        let head_pos = pos.relative(direction);
        if let Some(BlockEntity::Piston(moving)) = l.block_entity(head_pos).map(|entity| entity.clone()) {
            moving_piston::final_tick(l, head_pos, &moving);
        }
        let type_value = if sticky { "sticky" } else { "normal" };
        let moving_state = l.reg.default_state(moving_piston_block(l));
        let moving_state = l.reg.with_dir(moving_state, facing(l), direction);
        let moving_state = l.reg.with_name(moving_state, piston_type(l), type_value);
        l.set(pos, moving_state, 276);
        let moved = l.reg.with_dir(l.reg.default_state(l.block(state)), facing(l), from_3d_data_value(param & 7));
        l.set_block_entity(pos, BlockEntity::Piston(Moving::new(moved, direction, false, true)));
        l.update_neighbors_at(pos, l.block(moving_state));
        l.update_neighbour_shapes(moving_state, pos, 2, 512);
        if sticky {
            let block_pos = pos.relative_n(direction, 2);
            let block_state = l.get(block_pos);
            let mut flag = false;
            if l.block(block_state) == moving_piston_block(l) {
                if let Some(BlockEntity::Piston(moving)) = l.block_entity(block_pos).map(|entity| entity.clone()) {
                    if moving.direction() == direction && moving.is_extending() {
                        moving_piston::final_tick(l, block_pos, &moving);
                        flag = true;
                    }
                }
            }
            if !flag {
                if id != 1
                    || l.is_air(block_state)
                    || !is_pushable(l, block_state, block_pos, direction.opposite(), false, direction)
                    || push_reaction(l, block_state) != Push::Normal && l.block(block_state) != piston(l) && l.block(block_state) != sticky_piston(l)
                {
                    remove_block(l, pos.relative(direction), false);
                } else {
                    move_blocks(l, pos, direction, false, sticky);
                }
            }
        } else {
            let head_pos = pos.relative(direction);
            let head = l.reg.with_dir(l.reg.default_state(piston_head(l)), facing(l), direction);
            // Paper: a head that is not the plain one (a short head, say) is
            // only resent to clients.
            if l.get(head_pos) == head {
                remove_block(l, head_pos, false);
            }
        }
        // The sound's pitch.
        l.random.next_float();
        let event = crate::game_event!(l, "block_deactivate");
        l.game_event_in(event, pos, crate::game_event::Context::of_state(moving_state));
    }
    true
}

// Level.removeBlock: the block's fluid as a block.
pub fn remove_block(l: &mut Level, pos: Pos, moved_by_piston: bool) -> bool {
    l.remove_block(pos, moved_by_piston)
}

// WorldBorder.isWithinBounds at its default size: a static extent of
// 5.999997E7F around the origin, clamped to the absolute maximum.
fn within_border(pos: Pos) -> bool {
    const LIMIT: i32 = 29999984;
    pos.x() >= -LIMIT && pos.x() < LIMIT && pos.z() >= -LIMIT && pos.z() < LIMIT
}

// PistonBaseBlock.isPushable.
pub fn is_pushable(l: &Level, state: State, pos: Pos, movement_direction: Dir, allow_destroy: bool, piston_facing: Dir) -> bool {
    let max_y = MIN_Y + HEIGHT - 1;
    if pos.y() < MIN_Y || pos.y() > max_y || !within_border(pos) || !within_border(pos.relative(movement_direction)) {
        return false;
    }
    if l.is_air(state) {
        return true;
    }
    let block = l.block(state);
    if block == obsidian(l) || block == crying_obsidian(l) || block == respawn_anchor(l) || block == reinforced_deepslate(l) {
        return false;
    }
    if movement_direction == Dir::Down && pos.y() == MIN_Y {
        return false;
    }
    if movement_direction == Dir::Up && pos.y() == max_y {
        return false;
    }
    if block != piston(l) && block != sticky_piston(l) {
        if l.reg.states[state as usize].destroy_speed == -1.0 {
            return false;
        }
        match push_reaction(l, state) {
            Push::Block => return false,
            Push::Destroy => return allow_destroy,
            Push::PushOnly => return movement_direction == piston_facing,
            Push::Normal => {}
        }
    } else if l.reg.get_bool(state, extended(l)) {
        return false;
    }
    !l.has_block_entity(state)
}

// PistonBaseBlock.moveBlocks.
fn move_blocks(l: &mut Level, pos: Pos, facing_direction: Dir, extending: bool, sticky: bool) -> bool {
    let block_pos = pos.relative(facing_direction);
    if !extending {
        let head = l.get(block_pos);
        if l.block(head) == piston_head(l) {
            l.set(block_pos, l.known.air, 276);
        }
    }
    let mut resolver = Resolver::new(pos, facing_direction, extending);
    if !resolver.resolve(l) {
        return false;
    }
    // java.util.HashMap<BlockPos, BlockState>: its iteration order is the
    // order blocks are set to air in.
    let mut map: JavaHashMap<State> = JavaHashMap::new();
    let to_push = resolver.to_push.clone();
    for &pushed in &to_push {
        let state = l.get(pushed);
        map.put(pushed, state);
    }
    let to_destroy = resolver.to_destroy.clone();
    let mut block_states: Vec<State> = vec![0; to_push.len() + to_destroy.len()];
    let direction = if extending { facing_direction } else { facing_direction.opposite() };
    let mut i = 0;
    // The BlockPistonExtendEvent or BlockPistonRetractEvent: no listener
    // cancels it.
    for i1 in (0..to_destroy.len()).rev() {
        let destroyed = to_destroy[i1];
        let state = l.get(destroyed);
        drop_resources(l, state, destroyed);
        l.set(destroyed, l.known.air, 18);
        let event = crate::game_event!(l, "block_destroy");
        l.game_event_in(event, destroyed, crate::game_event::Context::of_state(state));
        block_states[i] = state;
        i += 1;
    }
    let moving_default = l.reg.default_state(moving_piston_block(l));
    for i1 in (0..to_push.len()).rev() {
        let old_pos = to_push[i1];
        let block_pos2 = old_pos.relative(direction);
        map.remove(block_pos2);
        let block_state2 = l.reg.with_dir(moving_default, facing(l), facing_direction);
        l.set_block(block_pos2, block_state2, 324, 512);
        let block_state1 = l.get(old_pos);
        map.replace(old_pos, block_state1);
        l.set_block_entity(block_pos2, BlockEntity::Piston(Moving::new(block_state1, facing_direction, extending, false)));
        l.set(old_pos, l.known.air, 594);
        block_states[i] = block_state1;
        i += 1;
    }
    if extending {
        let piston_type_value = if sticky { "sticky" } else { "normal" };
        let head = l.reg.default_state(piston_head(l));
        let head = l.reg.with_dir(head, facing(l), facing_direction);
        let head = l.reg.with_name(head, piston_type(l), piston_type_value);
        let moving_state = l.reg.with_dir(moving_default, facing(l), facing_direction);
        let moving_state = l.reg.with_name(moving_state, piston_type(l), piston_type_value);
        map.remove(block_pos);
        l.set_block(block_pos, moving_state, 324, 512);
        l.set_block_entity(block_pos, BlockEntity::Piston(Moving::new(head, facing_direction, true, true)));
    }
    let air = l.known.air;
    let entries = map.entries();
    for &(at, _) in &entries {
        l.set(at, air, 82);
    }
    for &(at, state) in &entries {
        super::update_indirect_neighbour_shapes(l, state, at, 2, 512);
        l.update_neighbour_shapes(air, at, 2, 512);
        super::update_indirect_neighbour_shapes(l, air, at, 2, 512);
    }
    // ExperimentalRedstoneUtils.initialOrientation: none without the
    // redstone experiments.
    i = 0;
    for i2 in (0..to_destroy.len()).rev() {
        let state = block_states[i];
        i += 1;
        let destroyed = to_destroy[i2];
        super::affect_neighbors_after_removal(l, state, destroyed, false);
        super::update_indirect_neighbour_shapes(l, state, destroyed, 2, 512);
        l.update_neighbors_at(destroyed, l.block(state));
    }
    for i2 in (0..to_push.len()).rev() {
        l.update_neighbors_at(to_push[i2], l.block(block_states[i]));
        i += 1;
    }
    if extending {
        l.update_neighbors_at(block_pos, piston_head(l));
    }
    true
}

// Paper's Block.dropResources(state, level, pos, blockEntity, source) with the
// block entity (getBlockEntity makes it if missing): the block's drops and
// experience (BlockBreakBlockEvent has no listener).
fn drop_resources(l: &mut Level, state: State, pos: Pos) {
    crate::entity::item::drop_resources_by_block(l, state, pos);
}

#[cfg(test)]
mod tests {
    use super::*;

    // The scenarios' generator (a 64-bit LCG), as the program that made
    // piston_hashmap.txt has it.
    struct Generator(u64);

    impl Generator {
        fn next(&mut self) -> i32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) as i32
        }

        fn pos(&mut self, base: (i32, i32, i32)) -> Pos {
            let x = base.0 + self.next() % 7 - 3;
            let y = base.1 + self.next() % 5 - 2;
            let z = base.2 + self.next() % 7 - 3;
            Pos::new(x, y, z)
        }
    }

    // JavaHashMap iterates as java.util.HashMap does: scenarios of puts,
    // removes and replaces, each with the order a JVM gave its entries
    // (piston_hashmap.txt is the output of a program that makes the same
    // scenarios with a HashMap keyed by BlockPos's hash).
    #[test]
    fn hash_map_iterates_as_java() {
        for (scenario, expected) in include_str!("piston_hashmap.txt").lines().enumerate() {
            let mut generator = Generator(scenario as u64);
            let base = (generator.next() % 2000 - 1000, generator.next() % 200, generator.next() % 2000 - 1000);
            let mut map = JavaHashMap::new();
            for value in 0..1 + generator.next() % 14 {
                let at = generator.pos(base);
                map.put(at, value as State);
            }
            for _ in 0..generator.next() % 4 {
                let at = generator.pos(base);
                map.remove(at);
            }
            for value in 0..generator.next() % 4 {
                let at = generator.pos(base);
                map.replace(at, 100 + value as State);
            }
            let got: String = map.entries().iter().map(|(at, value)| format!("{},{},{}:{};", at.x(), at.y(), at.z(), value)).collect();
            assert_eq!(got, expected, "scenario {scenario}");
        }
    }
}
