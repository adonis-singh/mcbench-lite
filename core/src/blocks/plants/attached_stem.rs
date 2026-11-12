// AttachedStemBlock: the stem turns back into a full-grown stem when the
// fruit it faces is gone.

use super::stem;
use crate::blocks::vegetation;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    vegetation::can_survive(l, pos, stem::may_place_on)
}

// AttachedStemBlock.updateShape: attached_<fruit>_stem has the fruit block
// <fruit> and the stem <fruit>_stem.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    let facing = l.reg.get_dir(state, property!(l, "facing", ["north", "south", "west", "east"]));
    let key = &l.reg.blocks[l.block(state) as usize].key;
    let fruit = key.strip_prefix("minecraft:attached_").and_then(|name| name.strip_suffix("_stem")).unwrap_or_else(|| panic!("attached stem {key}"));
    let fruit_block = l.reg.block_key(&format!("minecraft:{fruit}"));
    if l.block(neighbor) != fruit_block && direction == facing {
        // The stem's default state with its age set to 7 (trySetValue).
        let stem = l.reg.default_state(l.reg.block_key(&format!("minecraft:{fruit}_stem")));
        let age = l.reg.property_of(stem, "age");
        return l.reg.with_int(stem, age, 7);
    }
    vegetation::update_shape(l, state, pos, direction, neighbor_pos, neighbor)
}
