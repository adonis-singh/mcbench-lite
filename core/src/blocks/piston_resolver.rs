// PistonStructureResolver: the blocks a piston pushes and the blocks it
// destroys, up to 12 blocks, with slime and honey blocks pulling their
// neighbours along.

use super::piston::{self, is_pushable, push_reaction};
use crate::dir::{Dir, Pos, ALL};
use crate::level::Level;
use crate::registry::{Push, State};

const MAX_PUSH_DEPTH: usize = 12;

pub struct Resolver {
    piston_pos: Pos,
    extending: bool,
    start_pos: Pos,
    push_direction: Dir,
    piston_direction: Dir,
    pub to_push: Vec<Pos>,
    pub to_destroy: Vec<Pos>,
}

// PistonStructureResolver.isSticky.
fn is_sticky(l: &Level, state: State) -> bool {
    let block = l.block(state);
    block == piston::slime_block(l) || block == piston::honey_block(l)
}

// PistonStructureResolver.canStickToEachOther.
fn can_stick_to_each_other(l: &Level, state1: State, state2: State) -> bool {
    let (block1, block2) = (l.block(state1), l.block(state2));
    let (honey, slime) = (piston::honey_block(l), piston::slime_block(l));
    (block1 != honey || block2 != slime) && (block1 != slime || block2 != honey) && (is_sticky(l, state1) || is_sticky(l, state2))
}

impl Resolver {
    pub fn new(piston_pos: Pos, piston_direction: Dir, extending: bool) -> Resolver {
        let (push_direction, start_pos) = if extending {
            (piston_direction, piston_pos.relative(piston_direction))
        } else {
            (piston_direction.opposite(), piston_pos.relative_n(piston_direction, 2))
        };
        Resolver { piston_pos, extending, start_pos, push_direction, piston_direction, to_push: Vec::new(), to_destroy: Vec::new() }
    }

    pub fn resolve(&mut self, l: &mut Level) -> bool {
        self.to_push.clear();
        self.to_destroy.clear();
        let state = l.get(self.start_pos);
        if !is_pushable(l, state, self.start_pos, self.push_direction, false, self.piston_direction) {
            if self.extending && push_reaction(l, state) == Push::Destroy {
                self.to_destroy.push(self.start_pos);
                true
            } else {
                false
            }
        } else if !self.add_block_line(l, self.start_pos, self.push_direction) {
            false
        } else {
            // The list grows as sticky blocks add their branches.
            let mut i = 0;
            while i < self.to_push.len() {
                let pos = self.to_push[i];
                let state = l.get(pos);
                if is_sticky(l, state) && !self.add_branching_blocks(l, pos) {
                    return false;
                }
                i += 1;
            }
            true
        }
    }

    fn add_block_line(&mut self, l: &mut Level, origin_pos: Pos, direction: Dir) -> bool {
        let mut state = l.get(origin_pos);
        if l.is_air(state) {
            true
        } else if !is_pushable(l, state, origin_pos, self.push_direction, false, direction) {
            true
        } else if origin_pos == self.piston_pos {
            true
        } else if self.to_push.contains(&origin_pos) {
            true
        } else {
            let mut i = 1;
            if i + self.to_push.len() > MAX_PUSH_DEPTH {
                return false;
            }
            while is_sticky(l, state) {
                let pos = origin_pos.relative_n(self.push_direction.opposite(), i as i32);
                let state1 = state;
                state = l.get(pos);
                if l.is_air(state)
                    || !can_stick_to_each_other(l, state1, state)
                    || !is_pushable(l, state, pos, self.push_direction, false, self.push_direction.opposite())
                    || pos == self.piston_pos
                {
                    break;
                }
                i += 1;
                if i + self.to_push.len() > MAX_PUSH_DEPTH {
                    return false;
                }
            }
            let mut offsets = 0;
            for i2 in (0..i).rev() {
                self.to_push.push(origin_pos.relative_n(self.push_direction.opposite(), i2 as i32));
                offsets += 1;
            }
            let mut i2 = 1;
            loop {
                let pos1 = origin_pos.relative_n(self.push_direction, i2);
                if let Some(index) = self.to_push.iter().position(|&p| p == pos1) {
                    self.reorder_list_at_collision(offsets, index);
                    for i3 in 0..=index + offsets {
                        let pos2 = self.to_push[i3];
                        let state2 = l.get(pos2);
                        if is_sticky(l, state2) && !self.add_branching_blocks(l, pos2) {
                            return false;
                        }
                    }
                    return true;
                }
                state = l.get(pos1);
                if l.is_air(state) {
                    return true;
                }
                if !is_pushable(l, state, pos1, self.push_direction, true, self.push_direction) || pos1 == self.piston_pos {
                    return false;
                }
                if push_reaction(l, state) == Push::Destroy {
                    self.to_destroy.push(pos1);
                    return true;
                }
                if self.to_push.len() >= MAX_PUSH_DEPTH {
                    return false;
                }
                self.to_push.push(pos1);
                offsets += 1;
                i2 += 1;
            }
        }
    }

    fn reorder_list_at_collision(&mut self, offsets: usize, index: usize) {
        let len = self.to_push.len();
        let mut list = self.to_push[..index].to_vec();
        list.extend_from_slice(&self.to_push[len - offsets..]);
        list.extend_from_slice(&self.to_push[index..len - offsets]);
        self.to_push = list;
    }

    fn add_branching_blocks(&mut self, l: &mut Level, from_pos: Pos) -> bool {
        let state = l.get(from_pos);
        for direction in ALL {
            if direction.axis() != self.push_direction.axis() {
                let pos = from_pos.relative(direction);
                let state1 = l.get(pos);
                if can_stick_to_each_other(l, state1, state) && !self.add_block_line(l, pos, direction) {
                    return false;
                }
            }
        }
        true
    }
}
