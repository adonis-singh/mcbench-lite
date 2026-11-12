// BellBlock and BellBlockEntity, in a level that holds no entities: ringing
// (neighborChanged with a redstone signal) posts the ring block event, and
// the ring's entity searches (the nearby living entities that hear it, the
// raiders that glow) find none.

use super::entities::Entity;
use super::{BlockEntity, Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, UPDATE_ALL};
use crate::property;
use crate::registry::{BlockId, State, Support};

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(neighbor_changed),
    trigger_event: Some(trigger_event),
    can_survive: Some(can_survive),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    ..NONE
};

// BellBlockEntity's fields that change: lastRingTimestamp, ticks, shaking,
// whether nearbyEntities is set (null until the first ring is heard), and
// resonationTicks. It never resonates: that needs a raider nearby. The
// click direction is read only by the client's animation.
#[derive(Clone, Debug, Default)]
pub struct Bell {
    last_ring_timestamp: i64,
    ticks: i32,
    shaking: bool,
    nearby_entities_set: bool,
    resonation_ticks: i32,
}

// BellBlock.EVENT_BELL_RING.
const EVENT_BELL_RING: i32 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Attachment {
    Floor,
    Ceiling,
    SingleWall,
    DoubleWall,
}

impl Attachment {
    fn name(self) -> &'static str {
        match self {
            Attachment::Floor => "floor",
            Attachment::Ceiling => "ceiling",
            Attachment::SingleWall => "single_wall",
            Attachment::DoubleWall => "double_wall",
        }
    }
}

// BellBlock.ATTACHMENT, FACING and POWERED.
fn attachment_property(l: &Level) -> u16 {
    property!(l, "attachment", ["floor", "ceiling", "single_wall", "double_wall"])
}

fn facing_property(l: &Level) -> u16 {
    property!(l, "facing", ["north", "south", "west", "east"])
}

fn powered_property(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

fn attachment(l: &Level, state: State) -> Attachment {
    match l.reg.value_name(state, attachment_property(l)) {
        "floor" => Attachment::Floor,
        "ceiling" => Attachment::Ceiling,
        "single_wall" => Attachment::SingleWall,
        "double_wall" => Attachment::DoubleWall,
        other => panic!("no bell attachment {other}"),
    }
}

fn facing(l: &Level, state: State) -> Dir {
    l.reg.get_dir(state, facing_property(l))
}

// BellBlock.getConnectedDirection.
fn connected_direction(l: &Level, state: State) -> Dir {
    match attachment(l, state) {
        Attachment::Floor => Dir::Up,
        Attachment::Ceiling => Dir::Down,
        _ => facing(l, state).opposite(),
    }
}

// BellBlock.canSurvive: a bell hanging from the ceiling needs the block above
// to support its center (Block.canSupportCenter), any other needs the block
// it is attached to (the one below for a floor bell, the wall behind) to be
// sturdy toward it (FaceAttachedHorizontalDirectionalBlock.canAttach).
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let opposite = connected_direction(l, state).opposite();
    if opposite == Dir::Up {
        // Block.canSupportCenter(level, pos.above(), DOWN).
        let above = l.get(pos.above());
        !l.reg.in_tag(above, "minecraft:unstable_bottom_center") && crate::shape_ops::is_face_sturdy(l, above, pos.above(), Dir::Down, Support::Center)
    } else {
        // FaceAttachedHorizontalDirectionalBlock.canAttach.
        let attached = pos.relative(opposite);
        let neighbor = l.get(attached);
        crate::shape_ops::is_face_sturdy(l, neighbor, attached, opposite.opposite(), Support::Full)
    }
}

// BellBlock.updateShape; BlockBehaviour's returns the state.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    let attach = attachment(l, state);
    let opposite = connected_direction(l, state).opposite();
    if opposite == direction && !can_survive(l, state, pos) && attach != Attachment::DoubleWall {
        return l.known.air;
    }
    if direction.axis() == facing(l, state).axis() {
        if attach == Attachment::DoubleWall && !crate::shape_ops::is_face_sturdy(l, neighbor, neighbor_pos, direction, Support::Full) {
            let state = l.reg.with_name(state, attachment_property(l), Attachment::SingleWall.name());
            return l.reg.with_dir(state, facing_property(l), direction.opposite());
        }
        if attach == Attachment::SingleWall && opposite.opposite() == direction && crate::shape_ops::is_face_sturdy(l, neighbor, neighbor_pos, facing(l, state), Support::Full) {
            return l.reg.with_name(state, attachment_property(l), Attachment::DoubleWall.name());
        }
    }
    state
}

// BellBlock.neighborChanged: a rising signal rings the bell.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    let has_neighbor_signal = l.has_neighbor_signal(pos);
    if has_neighbor_signal != l.reg.get_bool(state, powered_property(l)) {
        if has_neighbor_signal {
            attempt_to_ring(l, pos);
        }
        l.set(pos, l.reg.with_bool(state, powered_property(l), has_neighbor_signal), UPDATE_ALL);
    }
}

// BellBlock.attemptToRing(level, pos, null): no entity, so the direction is
// the state's facing. The ring event has no listener; the sound reaches
// nothing.
fn attempt_to_ring(l: &mut Level, pos: Pos) {
    let is_bell = matches!(l.block_entity(pos), Some(BlockEntity::Entity(Entity::Bell(_))));
    if is_bell {
        let state = l.get(pos);
        let direction = facing(l, state);
        // BellBlockEntity.onHit.
        let block = l.block(state);
        let Some(BlockEntity::Entity(Entity::Bell(bell))) = l.block_entities.get_mut(&pos) else { unreachable!() };
        if bell.shaking {
            bell.ticks = 0;
        } else {
            bell.shaking = true;
        }
        l.block_event(pos, block, EVENT_BELL_RING, direction as i32);
        let event = crate::game_event!(l, "block_change");
        l.game_event_at(None, event, pos);
    }
}

// BaseEntityBlock.triggerEvent: Block's returns false, and the block entity
// (Level.getBlockEntity makes it if missing) answers. BellBlockEntity
// .triggerEvent: the ring.
fn trigger_event(l: &mut Level, _state: State, pos: Pos, id: i32, _param: i32) -> bool {
    let game_time = l.game_time;
    let Some(BlockEntity::Entity(Entity::Bell(bell))) = l.block_entity(pos) else { return false };
    if id != EVENT_BELL_RING {
        return false;
    }
    // updateEntities: the living entities within 48 blocks, searched again
    // after 60 ticks. There are none, so none hear it (HEARD_BELL_TIME) and
    // none are removed.
    if game_time > bell.last_ring_timestamp + 60 || !bell.nearby_entities_set {
        bell.last_ring_timestamp = game_time;
        bell.nearby_entities_set = true;
    }
    bell.resonation_ticks = 0;
    bell.ticks = 0;
    bell.shaking = true;
    true
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Entity(Entity::Bell(Bell::default()))
}

// BellBlock.getTicker on the server: BellBlockEntity.serverTick.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(server_tick)
}

// BellBlockEntity.tick. The nearby entities are none (and cleared), so no
// raider is ever nearby and the bell never resonates.
fn server_tick(l: &mut Level, pos: Pos, _state: State) {
    let Some(BlockEntity::Entity(Entity::Bell(bell))) = l.block_entities.get_mut(&pos) else {
        panic!("a bell ticks without its block entity")
    };
    if bell.shaking {
        bell.ticks += 1;
    }
    if bell.ticks >= 50 {
        bell.shaking = false;
        // nearbyEntities.clear() on a list that was never set throws.
        assert!(bell.nearby_entities_set, "a bell stops shaking before it heard a ring");
        bell.ticks = 0;
    }
    if bell.ticks >= 5 && bell.resonation_ticks == 0 {
        // areRaidersNearby over a list that was never set throws.
        assert!(bell.nearby_entities_set, "a bell checks raiders before it heard a ring");
    }
}
