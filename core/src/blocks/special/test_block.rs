// TestBlock and TestBlockEntity. A test block in the start mode gives a
// signal while it is powered, and one in another mode records that it was
// triggered; a log with a message writes it to the server's log (a new test
// block has none).

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::codec::{self, Input, R};
use crate::nbt::{Compound, Tag};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    neighbor_changed: Some(neighbor_changed),
    tick: Some(tick),
    get_signal: Some(get_signal),
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ..NONE
};

// TestBlockEntity: the mode it was made with, whether the block is powered
// and whether it was triggered.
#[derive(Clone, Debug)]
pub struct TestBlock {
    mode: Mode,
    powered: bool,
    triggered: bool,
    message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Start,
    Log,
    Fail,
    Accept,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Start => "start",
            Mode::Log => "log",
            Mode::Fail => "fail",
            Mode::Accept => "accept",
        }
    }
}

impl TestBlock {
    pub fn saved(&self) -> (&'static str, String) {
        let mut out = Compound::new();
        out.put_string("mode", self.mode.name());
        out.put_string("message", &self.message);
        out.put_boolean("powered", self.powered);
        out.put("components", Tag::Compound(Compound::new()));
        ("minecraft:test_block", out.to_string())
    }

    // TestBlockEntity.loadAdditional: the mode is fail where the data names none
    // (the block's state is not changed).
    pub fn load(&mut self, input: &Input) -> R<()> {
        self.mode = match input.read("mode", |tag| codec::string(tag).map(str::to_string))? {
            None => Mode::Fail,
            Some(name) => match name.as_str() {
                "start" => Mode::Start,
                "log" => Mode::Log,
                "fail" => Mode::Fail,
                "accept" => Mode::Accept,
                other => return codec::fail(format!("Unknown element name:{other}")),
            },
        };
        self.message = input.string("message")?.unwrap_or("").to_string();
        self.powered = input.bool_or("powered", false)?;
        Ok(())
    }
}

fn mode_property(l: &Level) -> u16 {
    property!(l, "mode", ["start", "log", "fail", "accept"])
}

fn state_mode(l: &Level, state: State) -> Mode {
    match l.reg.value_name(state, mode_property(l)) {
        "start" => Mode::Start,
        "log" => Mode::Log,
        "fail" => Mode::Fail,
        "accept" => Mode::Accept,
        other => panic!("no test block mode {other}"),
    }
}

fn new_block_entity(l: &mut Level, state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::TestBlock(TestBlock { mode: state_mode(l, state), powered: false, triggered: false, message: String::new() }))
}

fn entity(l: &mut Level, pos: Pos) -> Option<TestBlock> {
    match l.block_entity(pos) {
        Some(BlockEntity::Special(Special::TestBlock(test))) => Some(test.clone()),
        _ => None,
    }
}

fn store(l: &mut Level, pos: Pos, test: TestBlock) {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Special(Special::TestBlock(slot))) => *slot = test,
        _ => panic!("no test block entity at {pos}"),
    }
}

// TestBlock.tick: TestBlockEntity.reset.
fn tick(l: &mut Level, state: State, pos: Pos) {
    let Some(mut test) = entity(l, pos) else { return };
    test.triggered = false;
    if test.mode == Mode::Start {
        test.powered = false;
        store(l, pos, test);
        l.update_neighbors_at(pos, l.block(state));
    } else {
        store(l, pos, test);
    }
}

// TestBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, _state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    let Some(mut test) = entity(l, pos) else { return };
    if test.mode == Mode::Start {
        return;
    }
    let has_neighbor_signal = l.has_neighbor_signal(pos);
    if has_neighbor_signal && !test.powered {
        // setPowered(true) and trigger: in these modes it records the
        // trigger (and logs the message, which is blank).
        test.powered = true;
        test.triggered = true;
        store(l, pos, test);
    } else if !has_neighbor_signal && test.powered {
        test.powered = false;
        store(l, pos, test);
    }
}

// TestBlock.getSignal.
fn get_signal(l: &mut Level, state: State, pos: Pos, _dir: Dir) -> i32 {
    if state_mode(l, state) != Mode::Start {
        return 0;
    }
    match entity(l, pos) {
        Some(test) if test.powered => 15,
        _ => 0,
    }
}
