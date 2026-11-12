// TntBlock: primed when it gets power (onPlace, neighborChanged). Its
// player paths (flint and steel, breaking an unstable one, a burning
// projectile) are not inputs; being exploded is in explosion.rs.

use super::{Behaviour, NONE};
use crate::dir::Pos;
use crate::entity::tnt;
use crate::level::Level;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour { on_place: Some(on_place), neighbor_changed: Some(neighbor_changed), ..NONE };

fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    if l.block(old) != l.block(state) && l.has_neighbor_signal(pos) && tnt::prime(l, pos) {
        l.remove_block(pos, false);
    }
}

fn neighbor_changed(l: &mut Level, _state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    if l.has_neighbor_signal(pos) && tnt::prime(l, pos) {
        l.remove_block(pos, false);
    }
}
