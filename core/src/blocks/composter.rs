// ComposterBlock and the containers it hands hoppers (WorldlyContainerHolder).
// The block's useItemOn and useWithoutItem are a player's. The game events
// it posts (block change) reach nothing here.

use super::container::{self, CompostKind, Composter, Stack};
use super::{Behaviour, NONE};
use crate::dir::Pos;
use crate::game_event::Context;
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    on_place: Some(on_place),
    tick: Some(tick),
    get_analog_output_signal: Some(get_analog_output_signal),
    ..NONE
};

// ComposterBlock.LEVEL.
pub fn level_property(l: &Level) -> u16 {
    property!(l, "level", ["0", "1", "2", "3", "4", "5", "6", "7", "8"])
}

pub fn level_of(l: &Level, state: State) -> i32 {
    l.reg.get_int(state, level_property(l))
}

// ComposterBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    if level_of(l, state) == 7 {
        l.schedule_tick(pos, l.block(state), 20, PRIORITY_NORMAL);
    }
}

// ComposterBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if level_of(l, state) == 7 {
        // BlockState.cycle(LEVEL): the next value, wrapping to the first.
        let next = (level_of(l, state) + 1) % 9;
        l.set(pos, l.reg.with_int(state, level_property(l), next), UPDATE_ALL);
    }
}

// ComposterBlock.getAnalogOutputSignal.
fn get_analog_output_signal(l: &mut Level, state: State, _pos: Pos, _direction: crate::dir::Dir) -> i32 {
    level_of(l, state)
}

// ComposterBlock.COMPOSTABLES.containsKey(stack.getItem()).
pub fn compostable(l: &Level, stack: &Stack) -> bool {
    let s = stack.borrow();
    s.count > 0 && l.reg.items[s.item as usize].compost.is_some()
}

// stack.is(Items.BONE_MEAL).
pub fn is_bone_meal(l: &Level, stack: &Stack) -> bool {
    let s = stack.borrow();
    s.count > 0 && l.reg.items[s.item as usize].key == "minecraft:bone_meal"
}

// ComposterBlock.getContainer.
pub fn get_container(l: &Level, state: State, pos: Pos) -> Composter {
    let level = level_of(l, state);
    if level == 8 {
        let bone_meal = l.reg.item_by_key["minecraft:bone_meal"];
        Composter { pos, state, kind: CompostKind::Output, items: vec![container::new_stack(bone_meal, 1)], changed: false }
    } else if level < 7 {
        Composter { pos, state, kind: CompostKind::Input, items: vec![container::empty_stack()], changed: false }
    } else {
        Composter { pos, state, kind: CompostKind::Empty, items: vec![], changed: false }
    }
}

// ComposterBlock.addItem(entity null, state, level, pos, stack): the state
// the composter takes, or none if an event cancelled it (none does).
pub fn add_item(l: &mut Level, state: State, pos: Pos, stack: &Stack) -> Option<State> {
    // addItem(.., level.getRandom().nextDouble()).
    let rand = l.random.next_double();
    let level_value = level_of(l, state);
    let chance = l.reg.items[stack.borrow().item as usize].compost.unwrap_or(-1.0);
    // CompostItemEvent has no listener: willRaiseLevel stands.
    let will_raise_level = level_value == 0 && !(chance <= 0.0f32) || !(rand >= chance as f64);
    if !will_raise_level {
        return Some(state);
    }
    let i = level_value + 1;
    let new_state = l.reg.with_int(state, level_property(l), i);
    l.set(pos, new_state, UPDATE_ALL);
    l.game_event_in(crate::game_event!(l, "block_change"), pos, Context::of_state(new_state));
    if i == 7 {
        l.schedule_tick(pos, l.block(state), 20, PRIORITY_NORMAL);
    }
    Some(new_state)
}

// ComposterBlock.empty(null, state, level, pos).
pub fn empty(l: &mut Level, state: State, pos: Pos) -> State {
    let new_state = l.reg.with_int(state, level_property(l), 0);
    l.set(pos, new_state, UPDATE_ALL);
    l.game_event_in(crate::game_event!(l, "block_change"), pos, Context::of_state(new_state));
    new_state
}

// InputContainer.setChanged and OutputContainer.setChanged.
pub fn container_changed(l: &mut Level, c: &mut Composter) {
    match c.kind {
        CompostKind::Input => {
            let item = c.items[0].clone();
            if !container::is_empty(&item) {
                c.changed = true;
                let state = add_item(l, c.state, c.pos, &item);
                if state.is_none() {
                    return;
                }
                // level.levelEvent(1500, ..) reaches nothing; removeItemNoUpdate(0).
                c.items[0] = container::empty_stack();
            }
        }
        CompostKind::Output => {
            if c.items.iter().all(container::is_empty) {
                empty(l, c.state, c.pos);
                c.changed = true;
            } else {
                l.set(c.pos, c.state, UPDATE_ALL);
                c.changed = false;
            }
        }
        // SimpleContainer.setChanged tells its listeners: none.
        CompostKind::Empty => {}
    }
}

pub fn level(l: &Level, state: State) -> i32 {
    level_of(l, state)
}
