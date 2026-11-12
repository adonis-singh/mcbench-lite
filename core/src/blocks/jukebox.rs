// JukeboxBlock and JukeboxBlockEntity with its JukeboxSongPlayer: a record is
// a one slot container (ContainerSingleItem) that starts the song its item
// plays; the song player ticks while the block has a record. The level events
// (sounds, the music particles) reach nothing here; the game events and the
// random draw of the particles do.

use super::container::{self, is_empty, Stack};
use super::entities::{Entity, Jukebox};
use super::{BlockEntity, Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::game_event::Context;
use crate::level::{Level, UPDATE_CLIENTS};
use crate::loot::ItemStack;
use crate::nbt::Tag;
use crate::property;
use crate::registry::{Registry, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    affect_neighbors_after_removal: Some(container::affect_neighbors_after_removal),
    get_signal: Some(get_signal),
    get_analog_output_signal: Some(get_analog_output_signal),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    ..NONE
};

// JukeboxBlock.HAS_RECORD.
fn has_record(l: &Level) -> u16 {
    property!(l, "has_record", ["true", "false"])
}

fn jukebox_mut(l: &mut Level, pos: Pos) -> &mut Jukebox {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Entity(Entity::Jukebox(jukebox))) => jukebox,
        _ => panic!("a jukebox without its block entity"),
    }
}

// JukeboxSong.fromStack: the key of the song the stack's JUKEBOX_PLAYABLE
// names, if the registry has it.
pub fn song_of(reg: &Registry, stack: &ItemStack) -> Option<String> {
    if stack.count <= 0 {
        return None;
    }
    match stack.component(reg, "minecraft:jukebox_playable") {
        Some(Tag::String(key)) if reg.songs.contains_key(&key) => Some(key),
        Some(Tag::String(_)) => None,
        Some(_) => crate::level::refuse("a jukebox_playable that is no song key"),
        None => None,
    }
}

// JukeboxSong.lengthInTicks and hasFinished.
pub fn has_finished(reg: &Registry, song: &str, ticks: i64) -> bool {
    let seconds = reg.songs[song].length_in_seconds;
    // Mth.ceil(lengthInSeconds * 20.0F).
    let product = seconds * 20.0f32;
    let truncated = product as i32;
    let length = if product > truncated as f32 { truncated + 1 } else { truncated };
    ticks >= (length + 20) as i64
}

// JukeboxBlock.getSignal: 15 while the song player is playing. The block
// entity is looked up (Level.getBlockEntity makes it if missing).
fn get_signal(l: &mut Level, _state: State, pos: Pos, _direction: Dir) -> i32 {
    match l.block_entity(pos) {
        Some(BlockEntity::Entity(Entity::Jukebox(jukebox))) if jukebox.song.is_some() => 15,
        _ => 0,
    }
}

// JukeboxBlock.getAnalogOutputSignal: JukeboxBlockEntity.getComparatorOutput
// is the comparator output of the record's song, 0 for none.
fn get_analog_output_signal(l: &mut Level, _state: State, pos: Pos, _direction: Dir) -> i32 {
    let reg = l.reg;
    match l.block_entity(pos) {
        Some(BlockEntity::Entity(Entity::Jukebox(jukebox))) => {
            let item = jukebox.items[0].borrow().clone();
            song_of(reg, &item).map_or(0, |song| reg.songs[&song].comparator_output)
        }
        _ => 0,
    }
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Entity(Entity::Jukebox(Jukebox::new()))
}

// JukeboxBlock.getTicker: JukeboxBlockEntity.tick for a state with a record.
fn ticker(l: &Level, state: State) -> Option<fn(&mut Level, Pos, State)> {
    l.reg.get_bool(state, has_record(l)).then_some(tick as fn(&mut Level, Pos, State))
}

// JukeboxBlockEntity.tick: JukeboxSongPlayer.tick.
fn tick(l: &mut Level, pos: Pos, state: State) {
    let reg = l.reg;
    let jukebox = jukebox_mut(l, pos);
    let Some(song) = jukebox.song.clone() else { return };
    if has_finished(reg, &song, jukebox.ticks_since_song_started) {
        stop(l, pos, state);
    } else {
        if jukebox.ticks_since_song_started % 20 == 0 {
            let event = crate::game_event!(l, "jukebox_play");
            l.game_event_in(event, pos, Context::of_state(state));
            // spawnMusicParticles: the particle's colour is drawn from the level's random.
            l.random.next_int_bound(4);
        }
        jukebox_mut(l, pos).ticks_since_song_started += 1;
    }
}

// JukeboxBlockEntity.onSongChanged: the neighbours are updated (with the
// block of the entity's state) and the entity changed.
fn on_song_changed(l: &mut Level, pos: Pos, entity_state: State) {
    l.update_neighbors_at(pos, l.block(entity_state));
    container::set_changed_with_state(l, pos, entity_state);
}

// JukeboxSongPlayer.stop.
fn stop(l: &mut Level, pos: Pos, state: State) {
    let jukebox = jukebox_mut(l, pos);
    if jukebox.song.is_some() {
        jukebox.song = None;
        jukebox.ticks_since_song_started = 0;
        let event = crate::game_event!(l, "jukebox_stop_play");
        l.game_event_in(event, pos, Context::of_state(state));
        on_song_changed(l, pos, state);
    }
}

// JukeboxSongPlayer.play.
fn play(l: &mut Level, pos: Pos, song: String, entity_state: State) {
    let jukebox = jukebox_mut(l, pos);
    jukebox.song = Some(song);
    jukebox.ticks_since_song_started = 0;
    on_song_changed(l, pos, entity_state);
}

// JukeboxBlockEntity.setTheItem for the entity of the block that is in the
// world.
pub fn set_the_item(l: &mut Level, pos: Pos, stack: Stack) {
    let state = l.get(pos);
    set_the_item_in(l, pos, stack, state);
}

// JukeboxBlockEntity.setTheItem, the entity's own state being `entity_state`
// (the world's, except while its block is being replaced).
fn set_the_item_in(l: &mut Level, pos: Pos, stack: Stack, entity_state: State) {
    let reg = l.reg;
    jukebox_mut(l, pos).items[0] = stack.clone();
    let has = !is_empty(&stack);
    let song = song_of(reg, &stack.borrow());
    // notifyItemChangedInJukebox.
    let mut entity_state = entity_state;
    if l.get(pos) == entity_state {
        l.set(pos, reg.with_bool(entity_state, has_record(l), has), UPDATE_CLIENTS);
        // The entity's state is the block's now.
        entity_state = l.get(pos);
        let event = crate::game_event!(l, "block_change");
        l.game_event_in(event, pos, Context::of_state(entity_state));
    }
    match song {
        Some(song) if has => play(l, pos, song, entity_state),
        _ => stop(l, pos, entity_state),
    }
}

// JukeboxBlockEntity.splitTheItem (and removeTheItem): the record, the slot
// emptied.
pub fn split_the_item(l: &mut Level, pos: Pos) -> Stack {
    let item = jukebox_mut(l, pos).items[0].clone();
    set_the_item(l, pos, container::empty_stack());
    item
}

// JukeboxBlockEntity.canPlaceItem.
pub fn can_place_item(l: &Level, pos: Pos, slot: usize, stack: &Stack) -> bool {
    stack.borrow().component(l.reg, "minecraft:jukebox_playable").is_some() && is_empty(&container::slots(l, pos)[slot])
}

// JukeboxBlockEntity.popOutTheItem, for the entity of a block that is being
// replaced (its state the old one, the world's the new one): the record
// pops out as an item entity.
pub fn pop_out_the_item(l: &mut Level, pos: Pos, old: State) {
    let item = jukebox_mut(l, pos).items[0].clone();
    if is_empty(&item) {
        return;
    }
    // removeTheItem: splitTheItem(maxStackSize) takes the item out.
    set_the_item_in(l, pos, container::empty_stack(), old);
    // Vec3.atLowerCornerWithOffset(blockPos, 0.5, 1.01, 0.5).offsetRandom(random, 0.7F).
    let (x, y, z) = (pos.x() as f64 + 0.5, pos.y() as f64 + 1.01, pos.z() as f64 + 0.5);
    let dx = (l.random.next_float() - 0.5f32) * 0.7f32;
    let dy = (l.random.next_float() - 0.5f32) * 0.7f32;
    let dz = (l.random.next_float() - 0.5f32) * 0.7f32;
    let copy = container::copy(&item);
    let stack = copy.borrow().clone();
    crate::entity::item::spawn_with_default_delay(l, x + dx as f64, y + dy as f64, z + dz as f64, stack);
}
