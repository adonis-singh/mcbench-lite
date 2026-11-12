// Game events: GameEvent, GameEventDispatcher and the per-section listener
// registries (EuclideanGameEventListenerRegistry), ported from the pinned
// server's code. The only listeners in this level are block entities
// (sculk sensors, calibrated sculk sensors, sculk catalysts and sculk
// shriekers, which LevelChunk registers in the section they are in): there
// are no players, wardens or allays. An event reaches a listener within the
// listener's own radius; Paper's GenericGameEvent and BlockReceiveGameEvent
// have no handler.

use crate::blocks::{self, special};
use crate::dir::Pos;
use crate::entity::{Entity, Kind};
use crate::fast::FastMap;
use crate::level::Level;
use crate::phys::{self, Vec3};
use crate::registry::State;

pub type GameEvent = u16;

// A game event by its key without the namespace, looked up once per use.
#[macro_export]
macro_rules! game_event {
    ($l:expr, $name:literal) => {{
        static ID: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
        *ID.get_or_init(|| $l.reg.game_event_by_key[concat!("minecraft:", $name)])
    }};
}

// An entity as the source of an event: what the listeners ask of it (its
// uuid, the sneaking, spectator and vibration-dampening checks). Item,
// falling-block and primed-TNT entities exist: they are never spectators and
// never sneak, and an item dampens vibrations when its item is in the item
// tag that says so.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    pub id: i32,
    pub uuid: (i64, i64),
    pub dampens_vibrations: bool,
    // A LivingEntity (a stand is the only one here).
    pub living: bool,
}

impl Source {
    // The entity as the source of an event now (ItemEntity.dampensVibrations
    // reads its current item).
    pub fn of(l: &Level, e: &Entity) -> Source {
        match &e.kind {
            Kind::Item(d) => Source { id: e.id, uuid: e.uuid, dampens_vibrations: l.reg.item_tags["minecraft:dampens_vibrations"][d.stack.item as usize], living: false },
            // Entity.dampensVibrations is false for the others; none is a
            // spectator or sneaks.
            Kind::Falling(_) | Kind::Tnt(_) | Kind::Frame(_) | Kind::Minecart(_) | Kind::Boat(_) | Kind::Painting(_) | Kind::Orb(_) => Source { id: e.id, uuid: e.uuid, dampens_vibrations: false, living: false },
            Kind::ArmorStand(_) | Kind::Golem(_) => Source { id: e.id, uuid: e.uuid, dampens_vibrations: false, living: true },
        }
    }
}

// The input player's uuid (McbenchExperiment.newInputPlayer's game profile).
pub const INPUT_PLAYER_UUID: (i64, i64) = (0xbea4_4406_5db6_43c8_u64 as i64, 0xaa1b_d0f1_4f04_ac48_u64 as i64);

impl Source {
    // The player a "use" input is made by: a survival player in no world. It
    // never sneaks, is no spectator, and dampens no vibration. A vibration
    // keeps the entity that caused it (Paper's VibrationInfo.entity), so a
    // sensor that hears it passes the player on, as it does any entity.
    pub fn input_player(id: i32) -> Source {
        Source { id, uuid: INPUT_PLAYER_UUID, dampens_vibrations: false, living: true }
    }

    // Whether the source is the input player (a ServerPlayer, which a
    // shrieker answers to).
    pub fn is_input_player(&self) -> bool {
        self.uuid == INPUT_PLAYER_UUID
    }
}

// GameEvent.Context.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context {
    pub entity: Option<Source>,
    pub state: Option<State>,
}

impl Context {
    pub fn of_entity(entity: Option<Source>) -> Context {
        Context { entity, state: None }
    }

    pub fn of_state(state: State) -> Context {
        Context { entity: None, state: Some(state) }
    }

    pub fn of(entity: Option<Source>, state: State) -> Context {
        Context { entity, state: Some(state) }
    }
}

// The registries of the sections that hold a listener: a section's
// listeners in the order they registered (a listener is its block entity's
// position).
#[derive(Default)]
pub struct Listeners {
    sections: FastMap<(i32, i32, i32), Vec<Pos>>,
}

impl Listeners {
    fn section(pos: Pos) -> (i32, i32, i32) {
        (pos.x() >> 4, pos.y() >> 4, pos.z() >> 4)
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    // GameEventListenerRegistry.register.
    pub fn register(&mut self, pos: Pos) {
        self.sections.entry(Self::section(pos)).or_default().push(pos);
    }

    // GameEventListenerRegistry.unregister; the registry removes itself when
    // it is empty.
    pub fn unregister(&mut self, pos: Pos) {
        let key = Self::section(pos);
        let Some(list) = self.sections.get_mut(&key) else { return };
        if let Some(at) = list.iter().position(|&p| p == pos) {
            list.remove(at);
        }
        if list.is_empty() {
            self.sections.remove(&key);
        }
    }

    // Each section's listeners as McbenchEngine.state writes them: by the
    // chunk's key, then the section's y.
    pub fn state_lines(&self) -> Vec<String> {
        let mut keys: Vec<(i64, i32, i32, i32)> = self.sections.keys().map(|&(x, y, z)| (crate::dir::chunk_key(x, z), x, z, y)).collect();
        keys.sort();
        keys.into_iter()
            .map(|(_, x, z, y)| {
                let list: Vec<String> = self.sections[&(x, y, z)].iter().map(|p| format!("{},{},{}", p.x(), p.y(), p.z())).collect();
                format!("listeners {x} {z} {y}:{}", list.join(" "))
            })
            .collect()
    }
}

// Vec3.atCenterOf.
pub fn center_of(pos: Pos) -> Vec3 {
    Vec3::new(pos.x() as f64 + 0.5, pos.y() as f64 + 0.5, pos.z() as f64 + 0.5)
}

impl Level {
    // LevelAccessor.gameEvent(entity, event, pos): the event at the block's
    // centre, with the entity as its source.
    pub fn game_event_at(&mut self, entity: Option<Source>, event: GameEvent, pos: Pos) {
        self.game_event(event, center_of(pos), Context::of_entity(entity));
    }

    // LevelAccessor.gameEvent(event, pos, context).
    pub fn game_event_in(&mut self, event: GameEvent, pos: Pos, context: Context) {
        self.game_event(event, center_of(pos), context);
    }

    // ServerLevel.gameEvent, then GameEventDispatcher.post.
    pub fn game_event(&mut self, event: GameEvent, pos: Vec3, context: Context) {
        if !self.world.has_chunk(phys::floor(pos.x) >> 4, phys::floor(pos.z) >> 4) {
            return;
        }
        // Nothing hears it: the dispatcher only reads the registries.
        if self.listeners.is_empty() {
            return;
        }
        let radius = self.reg.game_events[event as usize].radius;
        let block = Pos::new(phys::floor(pos.x), phys::floor(pos.y), phys::floor(pos.z));
        let (min_x, min_y, min_z) = ((block.x() - radius) >> 4, (block.y() - radius) >> 4, (block.z() - radius) >> 4);
        let (max_x, max_y, max_z) = ((block.x() + radius) >> 4, (block.y() + radius) >> 4, (block.z() + radius) >> 4);
        for x in min_x..=max_x {
            for z in min_z..=max_z {
                if !self.world.has_chunk(x, z) {
                    continue;
                }
                for y in min_y..=max_y {
                    let Some(list) = self.listeners.sections.get(&(x, y, z)) else { continue };
                    let list = list.clone();
                    for listener in list {
                        // EuclideanGameEventListenerRegistry.getPostableListenerPosition.
                        let (dx, dy, dz) = ((listener.x() - block.x()) as f64, (listener.y() - block.y()) as f64, (listener.z() - block.z()) as f64);
                        let distance = dx * dx + dy * dy + dz * dz;
                        let reach = blocks::special::listener_radius(self, listener) as f64;
                        if distance > reach * reach {
                            continue;
                        }
                        special::handle_game_event(self, listener, event, context, pos);
                    }
                }
            }
        }
    }
}
