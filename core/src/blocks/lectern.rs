// LecternBlock and LecternBlockEntity: the book (and its page) that saved data
// or the verifier's book input gave it. A lectern placed by a command holds
// none, and a state placed with has_book still reads the empty book's signal.
// The book input places a book as the input player and takes one through
// the lectern's menu (try_place_book, take_book).

use super::container::{empty_stack, stack_of};
use super::entities::{Entity, Lectern};
use super::{BlockEntity, Behaviour, InputPlayer, NONE};
use crate::dir::{Dir, Pos};
use crate::game_event::{Context, Source};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::loot::ItemStack;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    tick: Some(tick),
    get_signal: Some(get_signal),
    get_direct_signal: Some(get_direct_signal),
    get_analog_output_signal: Some(get_analog_output_signal),
    new_block_entity: Some(new_block_entity),
    ..NONE
};

// LecternBlock.POWERED and HAS_BOOK.
fn powered(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

fn has_book(l: &Level) -> u16 {
    property!(l, "has_book", ["true", "false"])
}

// LecternBlock.updateBelow: without the redstone experiments the
// orientation is null and draws nothing from the level's random.
fn update_below(l: &mut Level, pos: Pos, state: State) {
    l.update_neighbors_at(pos.below(), l.block(state));
}

// LecternBlock.tick: changePowered(level, pos, state, false). The redstone
// change event has no listener: the new current stands. The block is set
// whether or not the state changes.
fn tick(l: &mut Level, state: State, pos: Pos) {
    l.set(pos, l.reg.with_bool(state, powered(l), false), UPDATE_ALL);
    update_below(l, pos, state);
}

// LecternBlockEntity.setPage, as the lectern's menu sets it: the page, clamped
// to the book's; a new page marks the entity changed (its comparators are told)
// and LecternBlock.signalPageChange powers the lectern for 2 ticks (the
// redstone change event has no listener; the block is set even when it is
// powered already; the level event sends nothing here).
pub fn set_page(l: &mut Level, pos: Pos, page: i32) {
    let Some(BlockEntity::Entity(Entity::Lectern(lectern))) = l.block_entity(pos) else { panic!("no lectern at {pos}") };
    let clamped = page.max(0).min(lectern.page_count - 1);
    if clamped == lectern.page {
        return;
    }
    lectern.page = clamped;
    super::container::be_changed(l, pos);
    let state = l.get(pos);
    l.set(pos, l.reg.with_bool(state, powered(l), true), UPDATE_ALL);
    update_below(l, pos, state);
    l.schedule_tick(pos, l.block(state), 2, PRIORITY_NORMAL);
}

// LecternBlock.resetBookState: the lectern unpowered, with or without its
// book, a block change by the entity given, and the block below told.
fn reset_book_state(l: &mut Level, entity: Option<Source>, pos: Pos, state: State, book: bool) {
    let block_state = l.reg.with_bool(l.reg.with_bool(state, powered(l), false), has_book(l), book);
    l.set(pos, block_state, UPDATE_ALL);
    let event = crate::game_event!(l, "block_change");
    l.game_event_in(event, pos, Context::of(entity, block_state));
    update_below(l, pos, state);
}

// LecternBlock.tryPlaceBook by the input player, with the state the lectern
// had when the input began: whether the lectern took the book (it does
// unless that state has a book). placeBook: the player is a ServerPlayer, so
// PlayerInsertLecternBookEvent is called (no listener) with a one-item copy
// of the stack, which the lectern takes; the player is in survival, so the
// stack shrinks by one. The lectern's block entity (Level.getBlockEntity
// makes it if missing) gets the book (setBook), the state is reset with the
// book by the player, and the BOOK_PUT sound plays (nothing here).
pub fn try_place_book(l: &mut Level, player: InputPlayer, pos: Pos, state: State, stack: &mut ItemStack) -> bool {
    if l.reg.get_bool(state, has_book(l)) {
        return false;
    }
    if let Some(BlockEntity::Entity(Entity::Lectern(_))) = l.block_entity(pos) {
        let mut book = stack.clone();
        book.count = 1;
        stack.count -= 1;
        set_book(l, pos, book);
        reset_book_state(l, Some(player.source), pos, state, true);
    }
    true
}

// LecternBlockEntity.setBook(stack, null): the book (resolving a written
// book's pages is off: Paper's item-validation resolve-selectors-in-books
// defaults to false), page 0, its page count, and the entity changed.
fn set_book(l: &mut Level, pos: Pos, book: ItemStack) {
    let reg = l.reg;
    let Some(BlockEntity::Entity(Entity::Lectern(lectern))) = l.block_entities.get_mut(&pos) else { panic!("no lectern at {pos}") };
    lectern.page_count = super::load::page_count(reg, &book);
    lectern.book = stack_of(book);
    lectern.page = 0;
    super::container::be_changed(l, pos);
}

// The take-book button of the lectern's menu (LecternMenu.clickMenuButton
// with BUTTON_TAKE_BOOK), for a player that may build: the
// PlayerTakeLecternBookEvent has no listener; LecternInventory's
// removeItemNoUpdate(0) takes the book out (onBookItemRemove: page and page
// count 0, the state reset without the book by no entity), and setChanged
// marks the lectern changed. The book is what the menu hands the player.
pub fn take_book(l: &mut Level, pos: Pos) -> ItemStack {
    let Some(BlockEntity::Entity(Entity::Lectern(lectern))) = l.block_entities.get_mut(&pos) else { panic!("no lectern at {pos}") };
    let book = std::mem::replace(&mut lectern.book, empty_stack());
    lectern.page = 0;
    lectern.page_count = 0;
    let state = l.get(pos);
    reset_book_state(l, None, pos, state, false);
    super::container::be_changed(l, pos);
    let book = book.borrow().clone();
    book
}

// LecternBlock.affectNeighborsAfterRemoval.
fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, _moved: bool) {
    if l.reg.get_bool(state, powered(l)) {
        update_below(l, pos, state);
    }
}

// LecternBlock.getSignal.
fn get_signal(l: &mut Level, state: State, _pos: Pos, _side: Dir) -> i32 {
    if l.reg.get_bool(state, powered(l)) {
        15
    } else {
        0
    }
}

// LecternBlock.getDirectSignal.
fn get_direct_signal(l: &mut Level, state: State, _pos: Pos, side: Dir) -> i32 {
    if side == Dir::Up && l.reg.get_bool(state, powered(l)) {
        15
    } else {
        0
    }
}

// LecternBlock.getAnalogOutputSignal: LecternBlockEntity.getRedstoneSignal
// (Level.getBlockEntity makes the entity if missing).
fn get_analog_output_signal(l: &mut Level, state: State, pos: Pos, _direction: Dir) -> i32 {
    if l.reg.get_bool(state, has_book(l)) {
        let reg = l.reg;
        if let Some(BlockEntity::Entity(Entity::Lectern(lectern))) = l.block_entity(pos) {
            return redstone_signal(reg, lectern);
        }
    }
    0
}

// LecternBlockEntity.hasBook: the book has writable or written book content.
pub fn has_book_content(reg: &crate::registry::Registry, lectern: &Lectern) -> bool {
    let book = lectern.book.borrow();
    book.count > 0 && (book.component(reg, "minecraft:writable_book_content").is_some() || book.component(reg, "minecraft:written_book_content").is_some())
}

// LecternBlockEntity.getRedstoneSignal.
fn redstone_signal(reg: &crate::registry::Registry, lectern: &Lectern) -> i32 {
    let f = if lectern.page_count > 1 { lectern.page as f32 / (lectern.page_count as f32 - 1.0f32) } else { 1.0f32 };
    (f * 14.0f32).floor() as i32 + has_book_content(reg, lectern) as i32
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Entity(Entity::Lectern(Lectern::new()))
}
