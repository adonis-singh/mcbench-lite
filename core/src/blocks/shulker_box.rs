// ShulkerBoxBlock and ShulkerBoxBlockEntity. Nothing in this level opens a
// shulker box (a player does, through startOpen), so its lid stays closed;
// the animation is ported whole all the same.

use super::container::{self, Container, Kind};
use super::{BlockEntity, Behaviour, NONE};
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    affect_neighbors_after_removal: Some(container::affect_neighbors_after_removal),
    get_analog_output_signal: Some(container::analog_output_signal),
    trigger_event: Some(container::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    ..NONE
};

// ShulkerBoxBlockEntity.AnimationStatus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationStatus {
    Closed,
    Opening,
    Opened,
    Closing,
}

// ShulkerBoxBlockEntity's openCount, animationStatus, progress and
// progressOld; its colour is read only for drops.
#[derive(Clone, Debug)]
pub struct ShulkerBox {
    // Written by triggerEvent and read only by startOpen (a player).
    #[allow(dead_code)]
    open_count: i32,
    animation_status: AnimationStatus,
    progress: f32,
    progress_old: f32,
}

impl ShulkerBox {
    // ShulkerBoxBlockEntity.triggerEvent.
    pub fn trigger_event(&mut self, id: i32, param: i32) -> bool {
        if id == 1 {
            self.open_count = param;
            if param == 0 {
                self.animation_status = AnimationStatus::Closing;
            }
            if param == 1 {
                // An opening lid changes the box's shape, and with it what
                // neighbours read of its faces (Registry::sturdy).
                crate::level::refuse("a shulker box lid opens");
            }
            true
        } else {
            false
        }
    }
}

// ShulkerBoxBlock.newBlockEntity.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Container(Container::new(Kind::ShulkerBox(ShulkerBox { open_count: 0, animation_status: AnimationStatus::Closed, progress: 0.0, progress_old: 0.0 })))
}

// ShulkerBoxBlock.getTicker: ShulkerBoxBlockEntity.tick.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(tick)
}

fn get(l: &Level, pos: Pos) -> ShulkerBox {
    match l.block_entities.get(&pos) {
        Some(BlockEntity::Container(Container { kind: Kind::ShulkerBox(shulker), .. })) => shulker.clone(),
        _ => panic!("a shulker box ticks without its block entity"),
    }
}

fn put(l: &mut Level, pos: Pos, shulker: &ShulkerBox) {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Container(Container { kind: Kind::ShulkerBox(entity), .. })) => *entity = shulker.clone(),
        _ => panic!("a shulker box ticks without its block entity"),
    }
}

// ShulkerBoxBlockEntity.tick, which is updateAnimation.
fn tick(l: &mut Level, pos: Pos, state: State) {
    let mut shulker = get(l, pos);
    shulker.progress_old = shulker.progress;
    match shulker.animation_status {
        AnimationStatus::Closed => shulker.progress = 0.0,
        AnimationStatus::Opening => {
            shulker.progress += 0.1;
            put(l, pos, &shulker);
            if shulker.progress_old == 0.0 {
                do_neighbor_updates(l, pos, state);
            }
            if shulker.progress >= 1.0 {
                shulker.animation_status = AnimationStatus::Opened;
                shulker.progress = 1.0;
                put(l, pos, &shulker);
                do_neighbor_updates(l, pos, state);
            }
            // moveCollidedEntities: no entity is in the lid's way.
        }
        AnimationStatus::Opened => shulker.progress = 1.0,
        AnimationStatus::Closing => {
            shulker.progress -= 0.1;
            put(l, pos, &shulker);
            if shulker.progress_old == 1.0 {
                do_neighbor_updates(l, pos, state);
            }
            if shulker.progress <= 0.0 {
                shulker.animation_status = AnimationStatus::Closed;
                shulker.progress = 0.0;
                put(l, pos, &shulker);
                do_neighbor_updates(l, pos, state);
            }
        }
    }
    put(l, pos, &shulker);
}

// ShulkerBoxBlockEntity.doNeighborUpdates.
fn do_neighbor_updates(l: &mut Level, pos: Pos, state: State) {
    l.update_neighbour_shapes(state, pos, 3, 512);
    l.update_neighbors_at(pos, l.block(state));
}
