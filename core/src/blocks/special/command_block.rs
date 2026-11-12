// CommandBlock and CommandBlockEntity (BaseCommandBlock). Blocks are placed
// without NBT, so every command is empty: a command block never executes a
// command, and a chain of them only passes the signal along, as success
// counts and last execution times.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::codec::{Input, R};
use crate::dir::{Dir, Pos};
use crate::nbt::{Compound, Tag};
use crate::registry::Registry;
use crate::level::{refuse, Level, PRIORITY_NORMAL};
use crate::property;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    neighbor_changed: Some(neighbor_changed),
    tick: Some(tick),
    get_analog_output_signal: Some(get_analog_output_signal),
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ..NONE
};

// CommandBlockEntity (with its BaseCommandBlock's command, name, success count,
// output and the game time it last ran). A command is not run here: a block
// that performs one other than "Searge" (whose answer is its easter egg)
// refuses.
#[derive(Clone, Debug)]
pub struct CommandBlock {
    powered: bool,
    auto: bool,
    condition_met: bool,
    success_count: i32,
    last_execution: i64,
    command: String,
    // BaseCommandBlock.customName and lastOutput, as the tags their codec writes.
    custom_name: Option<Tag>,
    track_output: bool,
    last_output: Option<Tag>,
    update_last_execution: bool,
    // Set by a load that made the entity automatic: the tick it schedules once
    // the entity is stored (see after_load).
    schedule_after_load: bool,
}

impl CommandBlock {
    // The entity's saved data (BaseCommandBlock.save, then the entity's flags).
    pub fn saved(&self) -> (&'static str, String) {
        let mut out = Compound::new();
        out.put_string("Command", &self.command);
        out.put_int("SuccessCount", self.success_count);
        if let Some(name) = &self.custom_name {
            out.put("CustomName", name.clone());
        }
        out.put_boolean("TrackOutput", self.track_output);
        if self.track_output {
            if let Some(output) = &self.last_output {
                out.put("LastOutput", output.clone());
            }
        }
        out.put_boolean("UpdateLastExecution", self.update_last_execution);
        if self.update_last_execution && self.last_execution != -1 {
            out.put("LastExecution", Tag::Long(self.last_execution));
        }
        out.put_boolean("powered", self.powered);
        out.put_boolean("conditionMet", self.condition_met);
        out.put_boolean("auto", self.auto);
        out.put("components", Tag::Compound(Compound::new()));
        ("minecraft:command_block", out.to_string())
    }

    // BaseCommandBlock.load, then the entity's flags (loadAdditional); whether
    // setAutomatic(true) has a tick to schedule (the entity was not automatic,
    // is not powered and its block is not a chain).
    pub fn load(&mut self, reg: &Registry, input: &Input) -> R<()> {
        self.command = input.string("Command")?.unwrap_or("").to_string();
        self.success_count = input.int_or("SuccessCount", 0)?;
        self.custom_name = input.read("CustomName", |tag| crate::text::component(reg, tag))?;
        self.track_output = input.bool_or("TrackOutput", true)?;
        self.last_output = if self.track_output { input.read("LastOutput", |tag| crate::text::component(reg, tag))? } else { None };
        self.update_last_execution = input.bool_or("UpdateLastExecution", true)?;
        if self.update_last_execution {
            self.last_execution = input.long_or("LastExecution", -1)?;
        } else {
            self.last_execution = -1;
        }
        self.powered = input.bool_or("powered", false)?;
        self.condition_met = input.bool_or("conditionMet", false)?;
        let was_auto = self.auto;
        self.auto = input.bool_or("auto", false)?;
        self.schedule_after_load = !was_auto && self.auto && !self.powered;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Sequence,
    Auto,
    Redstone,
}

fn facing(l: &Level) -> u16 {
    property!(l, "facing", ["north", "east", "south", "west", "up", "down"])
}

fn conditional(l: &Level) -> u16 {
    property!(l, "conditional", ["true", "false"])
}

// CommandBlockEntity.getMode, from the block the entity is in.
fn mode(l: &Level, state: State) -> Mode {
    match l.reg.blocks[l.block(state) as usize].key.as_str() {
        "minecraft:command_block" => Mode::Redstone,
        "minecraft:repeating_command_block" => Mode::Auto,
        "minecraft:chain_command_block" => Mode::Sequence,
        other => panic!("{other} is no command block"),
    }
}

// CommandBlock.newBlockEntity: automatic as the block is (setAutomatic runs
// with no level, so it schedules nothing).
fn new_block_entity(l: &mut Level, state: State, _pos: Pos) -> BlockEntity {
    let automatic = l.reg.automatic[&l.block(state)];
    BlockEntity::Special(Special::CommandBlock(CommandBlock {
        powered: false,
        auto: automatic,
        condition_met: false,
        success_count: 0,
        last_execution: -1,
        command: String::new(),
        custom_name: None,
        track_output: true,
        last_output: None,
        update_last_execution: true,
        schedule_after_load: false,
    }))
}

// The command block entity at the position (Level.getBlockEntity makes it if
// missing), cloned.
fn entity(l: &mut Level, pos: Pos) -> Option<CommandBlock> {
    match l.block_entity(pos) {
        Some(BlockEntity::Special(Special::CommandBlock(command))) => Some(command.clone()),
        _ => None,
    }
}

fn store(l: &mut Level, pos: Pos, command: CommandBlock) {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Special(Special::CommandBlock(slot))) => *slot = command,
        _ => panic!("no command block entity at {pos}"),
    }
}

// CommandBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    if entity(l, pos).is_some() {
        let powered = l.has_neighbor_signal(pos);
        set_powered_and_update(l, state, pos, powered);
    }
}

// CommandBlock.setPoweredAndUpdate. The redstone change event has no
// listener: the new current stands.
fn set_powered_and_update(l: &mut Level, state: State, pos: Pos, powered: bool) {
    let mut command = entity(l, pos).expect("a command block entity");
    if powered != command.powered {
        command.powered = powered;
        store(l, pos, command.clone());
        if powered {
            if command.auto || mode(l, state) == Mode::Sequence {
                return;
            }
            mark_condition_met(l, pos);
            l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
        }
    }
}

// CommandBlockEntity.isConditional: from the block at its position.
fn is_conditional(l: &mut Level, pos: Pos) -> bool {
    let state = l.get(pos);
    crate::shape_classes!(l, ["CommandBlock"])[l.block(state) as usize] && l.reg.get_bool(state, conditional(l))
}

// CommandBlockEntity.markConditionMet.
fn mark_condition_met(l: &mut Level, pos: Pos) -> bool {
    let mut met = true;
    if is_conditional(l, pos) {
        let state = l.get(pos);
        let back = pos.relative(l.reg.get_dir(state, facing(l)).opposite());
        let back_state = l.get(back);
        if crate::shape_classes!(l, ["CommandBlock"])[l.block(back_state) as usize] {
            met = entity(l, back).is_some_and(|command| command.success_count > 0);
        } else {
            met = false;
        }
    }
    let mut command = entity(l, pos).expect("a command block entity");
    command.condition_met = met;
    store(l, pos, command);
    met
}

// BaseCommandBlock.setSuccessCount.
fn set_success_count(l: &mut Level, pos: Pos, count: i32) {
    let mut command = entity(l, pos).expect("a command block entity");
    command.success_count = count;
    store(l, pos, command);
}

// CommandBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    let Some(command) = entity(l, pos) else { return };
    let mode = mode(l, state);
    let was_condition_met = command.condition_met;
    if mode == Mode::Auto {
        mark_condition_met(l, pos);
        if was_condition_met {
            execute(l, state, pos);
        } else if is_conditional(l, pos) {
            set_success_count(l, pos, 0);
        }
        let command = entity(l, pos).expect("a command block entity");
        if command.powered || command.auto {
            l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
        }
    } else if mode == Mode::Redstone {
        if was_condition_met {
            execute(l, state, pos);
        } else if is_conditional(l, pos) {
            set_success_count(l, pos, 0);
        }
    }
    l.update_neighbour_for_output_signal(pos, l.block(state));
}

// CommandBlock.execute: a command that is not empty is performed, an empty one
// leaves the success count 0.
fn execute(l: &mut Level, state: State, pos: Pos) {
    let command = entity(l, pos).expect("a command block entity");
    if !command.command.is_empty() {
        perform_command(l, pos);
    } else {
        set_success_count(l, pos, 0);
    }
    execute_chain(l, pos, l.reg.get_dir(state, facing(l)));
}

// BaseCommandBlock.performCommand: once per game tick; the command "Searge"
// answers its easter egg; an empty command only sets the success count to 0 and
// remembers the game time; any other would run (the commandBlocksEnabled game
// rule is on).
fn perform_command(l: &mut Level, pos: Pos) -> bool {
    let mut command = entity(l, pos).expect("a command block entity");
    if l.game_time == command.last_execution {
        return false;
    }
    if is_searge(&command.command) {
        command.last_output = Some(Tag::string("#itzlipofutzli"));
        command.success_count = 1;
        store(l, pos, command);
        return true;
    }
    if !command.command.is_empty() {
        refuse(format!("the command block at {pos} runs a command"));
    }
    command.success_count = 0;
    command.last_execution = if command.update_last_execution { l.game_time } else { -1 };
    store(l, pos, command);
    true
}

// "Searge".equalsIgnoreCase(command): each letter by Character.toUpperCase, so
// the long s (which upper cases to S) counts as an s.
fn is_searge(command: &str) -> bool {
    const LETTERS: [&str; 6] = ["sSſ", "eE", "aA", "rR", "gG", "eE"];
    let chars: Vec<char> = command.chars().collect();
    chars.len() == 6 && chars.iter().zip(LETTERS).all(|(c, set)| set.contains(*c))
}

// CommandBlockEntity.loadAdditional's setAutomatic(true) on an entity that
// was not automatic: scheduleTick (markConditionMet, then the block's tick).
pub fn after_load(l: &mut Level, pos: Pos) {
    let Some(BlockEntity::Special(Special::CommandBlock(command))) = l.block_entities.get(&pos) else { return };
    if !command.schedule_after_load {
        return;
    }
    let state = l.get(pos);
    if mode(l, state) == Mode::Sequence {
        return;
    }
    mark_condition_met(l, pos);
    l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
    let mut command = entity(l, pos).expect("a command block entity");
    command.schedule_after_load = false;
    store(l, pos, command);
}

// CommandBlock.executeChain, with the game rule maxCommandChainLength at its
// default.
fn execute_chain(l: &mut Level, pos: Pos, mut direction: Dir) {
    let mut at = pos;
    let mut left = 65536;
    while left > 0 {
        left -= 1;
        at = at.relative(direction);
        let state = l.get(at);
        if l.reg.blocks[l.block(state) as usize].key != "minecraft:chain_command_block" {
            break;
        }
        let Some(command) = entity(l, at) else { break };
        if mode(l, state) != Mode::Sequence {
            break;
        }
        if command.powered || command.auto {
            if mark_condition_met(l, at) {
                if !perform_command(l, at) {
                    break;
                }
                l.update_neighbour_for_output_signal(at, l.block(state));
            } else if is_conditional(l, at) {
                set_success_count(l, at, 0);
            }
        }
        direction = l.reg.get_dir(state, facing(l));
    }
}

// CommandBlock.getAnalogOutputSignal.
fn get_analog_output_signal(l: &mut Level, _state: State, pos: Pos, _dir: Dir) -> i32 {
    entity(l, pos).map_or(0, |command| command.success_count)
}
