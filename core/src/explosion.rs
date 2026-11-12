// ServerExplosion for a primed TNT's explosion (Level.ExplosionInteraction.TNT
// with the game rule tntExplosionDropDecay off: blocks are destroyed and
// drop what their loot tables give, with no explosion radius), ported from
// Paper's ServerExplosion and moonrise's ExplosionBlockCache. Paper's world
// configuration is at its defaults: optimize-explosions off,
// disable-explosion-knockback off, allow-headless-pistons off,
// allow-permanent-block-break-exploits off.
//
// The Bukkit events (EntityExplodeEvent, TNTPrimeEvent, the damage and
// knockback events) have no listener: their results stand, but the block list
// EntityExplodeEvent is given is the shuffled one reversed, with air left out.

use crate::blocks::{moving_piston, piston, BlockEntity};
use crate::clip::shape_clip_hits;
use crate::collision::{shape_of, Ctx};
use crate::dir::Pos;
use crate::entity::{self, entities_in, falling::is_destroyable, item, Entity, EntityId, Kind};
use crate::level::{refuse, Level, UPDATE_ALL};
use crate::loot::ItemStack;
use crate::phys::{self, Aabb, Vec3};
use crate::registry::{Fluid, State, OFFSET};
use crate::world::{HEIGHT, MIN_Y};
use std::collections::HashMap;
use std::sync::OnceLock;

// ServerExplosion.CACHED_RAYS: the direction of each ray, scaled to 0.3, three
// doubles per ray.
fn rays() -> &'static [f64] {
    static RAYS: OnceLock<Vec<f64>> = OnceLock::new();
    RAYS.get_or_init(|| {
        let mut out = Vec::new();
        for x in 0..=15 {
            for y in 0..=15 {
                for z in 0..=15 {
                    if x == 0 || x == 15 || y == 0 || y == 15 || z == 0 || z == 15 {
                        let x_dir = ((x as f32) / 15.0f32 * 2.0f32 - 1.0f32) as f64;
                        let y_dir = ((y as f32) / 15.0f32 * 2.0f32 - 1.0f32) as f64;
                        let z_dir = ((z as f32) / 15.0f32 * 2.0f32 - 1.0f32) as f64;
                        let mag = (x_dir * x_dir + y_dir * y_dir + z_dir * z_dir).sqrt();
                        out.push(x_dir / mag * 0.3f32 as f64);
                        out.push(y_dir / mag * 0.3f32 as f64);
                        out.push(z_dir / mag * 0.3f32 as f64);
                    }
                }
            }
        }
        out
    })
}

// ExplosionBlockCache.
struct Cached {
    // None where the position is outside the world.
    state: Option<State>,
    resistance: f32,
    should_explode: Option<bool>,
}

struct Explosion {
    center: Vec3,
    radius: f32,
    fire: bool,
    // The source is a primed TNT minecart: rails (and what lies under one) do
    // not resist it nor explode (MinecartTNT.getBlockExplosionResistance and
    // shouldBlockExplode).
    primed_cart: bool,
    cache: HashMap<(i32, i32, i32), Cached>,
}

// WorldBorder at its default size: far beyond anything a build reaches.
const BORDER: i32 = 29_000_000;

impl Explosion {
    // getOrCacheExplosionBlock: the block cached for a position, made if new.
    fn block(&mut self, l: &mut Level, x: i32, y: i32, z: i32, calculate_resistance: bool) -> &mut Cached {
        if !self.cache.contains_key(&(x, y, z)) {
            if x.abs() > BORDER || z.abs() > BORDER {
                refuse("an explosion near the world border");
            }
            let entry = if y < MIN_Y || y >= MIN_Y + HEIGHT {
                Cached { state: None, resistance: 0.0, should_explode: None }
            } else {
                let state = l.get(Pos::new(x, y, z));
                let fluid = l.fluid_type(l.fluid_of(state));
                // The damage calculator: EntityBasedExplosionDamageCalculator over
                // ExplosionDamageCalculator. Air without fluid has no resistance;
                // a fluid's is its block's (water and lava: 100).
                let resistance = if !calculate_resistance || (l.is_air(state) && fluid == Fluid::Empty) {
                    None
                } else {
                    let fluid_resistance = match fluid {
                        Fluid::Empty => 0.0f32,
                        Fluid::FlowingWater | Fluid::Water => l.reg.blocks[l.reg.block_key("minecraft:water") as usize].explosion_resistance,
                        Fluid::FlowingLava | Fluid::Lava => l.reg.blocks[l.reg.block_key("minecraft:lava") as usize].explosion_resistance,
                    };
                    let resistance = l.reg.blocks[l.block(state) as usize].explosion_resistance.max(fluid_resistance);
                    // EntityBasedExplosionDamageCalculator: the source's say on it.
                    Some(if self.primed_cart && self.rails_at_or_above(l, state, x, y, z) { 0.0f32 } else { resistance })
                };
                Cached { state: Some(state), resistance: (resistance.unwrap_or(-0.3f32) + 0.3f32) * 0.3f32, should_explode: None }
            };
            self.cache.insert((x, y, z), entry);
        }
        self.cache.get_mut(&(x, y, z)).unwrap()
    }

    // The block is a rail or has a rail above it.
    fn rails_at_or_above(&self, l: &mut Level, state: State, x: i32, y: i32, z: i32) -> bool {
        if l.reg.in_tag(state, "minecraft:rails") {
            return true;
        }
        let above = l.get(Pos::new(x, y + 1, z));
        l.reg.in_tag(above, "minecraft:rails")
    }

    // calculateExplodedPositions.
    fn exploded_positions(&mut self, l: &mut Level) -> Vec<Pos> {
        let mut out = Vec::new();
        let center = self.center;
        let (bx, by, bz) = (phys::floor(center.x), phys::floor(center.y), phys::floor(center.z));
        self.block(l, bx, by, bz, true);
        let rays = rays();
        let mut i = 0;
        while i < rays.len() {
            let (inc_x, inc_y, inc_z) = (rays[i], rays[i + 1], rays[i + 2]);
            i += 3;
            let (mut x, mut y, mut z) = (center.x, center.y, center.z);
            let mut power = self.radius * (0.7f32 + l.random.next_float() * 0.6f32);
            loop {
                let (block_x, block_y, block_z) = (phys::floor(x), phys::floor(y), phys::floor(z));
                let radius_fire = self.fire;
                let cached = self.block(l, block_x, block_y, block_z, true);
                let Some(state) = cached.state else { break };
                power -= cached.resistance;
                if power > 0.0f32 && cached.should_explode.is_none() {
                    // isDestroyable && the source's shouldBlockExplode.
                    let should = is_destroyable(l, state) && !(self.primed_cart && self.rails_at_or_above(l, state, block_x, block_y, block_z));
                    self.block(l, block_x, block_y, block_z, true).should_explode = Some(should);
                    if should && (radius_fire || !l.is_air(state)) {
                        let pos = Pos::new(block_x, block_y, block_z);
                        out.push(pos);
                        // Paper (allow-headless-pistons off): a piston's own
                        // moving part (its head going out, its base going in)
                        // adds the block behind it, the side its block's
                        // facing points away from, even where that is
                        // already listed.
                        if moving_piston::is_moving_piston(l, state) {
                            if let Some(BlockEntity::Piston(moving)) = l.block_entity(pos) {
                                if moving.is_source_piston() {
                                    let facing = l.reg.get_dir(state, piston::facing(l));
                                    out.push(pos.relative(facing.opposite()));
                                }
                            }
                        }
                    }
                }
                power -= 0.22500001f32;
                x += inc_x;
                y += inc_y;
                z += inc_z;
                if !(power > 0.0f32) {
                    break;
                }
            }
        }
        out
    }

    // clipsAnything: whether a block's collision shape stops the segment.
    fn clips_anything(&mut self, l: &mut Level, ctx: Ctx, from: Vec3, to: Vec3) -> bool {
        let adj_x = 1.0E-7 * (from.x - to.x);
        let adj_y = 1.0E-7 * (from.y - to.y);
        let adj_z = 1.0E-7 * (from.z - to.z);
        if adj_x == 0.0 && adj_y == 0.0 && adj_z == 0.0 {
            return false;
        }
        let (to_x, to_y, to_z) = (to.x - adj_x, to.y - adj_y, to.z - adj_z);
        let (from_x, from_y, from_z) = (from.x + adj_x, from.y + adj_y, from.z + adj_z);
        let (mut cur_x, mut cur_y, mut cur_z) = (phys::floor(from_x), phys::floor(from_y), phys::floor(from_z));
        let (diff_x, diff_y, diff_z) = (to_x - from_x, to_y - from_y, to_z - from_z);
        let (dx_double, dy_double, dz_double) = (signum(diff_x), signum(diff_y), signum(diff_z));
        let (dx, dy, dz) = (dx_double as i32, dy_double as i32, dz_double as i32);
        let norm_diff_x = if diff_x == 0.0 { f64::MAX } else { dx_double / diff_x };
        let norm_diff_y = if diff_y == 0.0 { f64::MAX } else { dy_double / diff_y };
        let norm_diff_z = if diff_z == 0.0 { f64::MAX } else { dz_double / diff_z };
        let mut norm_x = norm_diff_x * if diff_x > 0.0 { 1.0 - phys::frac(from_x) } else { phys::frac(from_x) };
        let mut norm_y = norm_diff_y * if diff_y > 0.0 { 1.0 - phys::frac(from_y) } else { phys::frac(from_y) };
        let mut norm_z = norm_diff_z * if diff_z > 0.0 { 1.0 - phys::frac(from_z) } else { phys::frac(from_z) };
        loop {
            let cached = self.block(l, cur_x, cur_y, cur_z, false);
            if let Some(state) = cached.state {
                let shape = shape_of(l, state, Pos::new(cur_x, cur_y, cur_z), ctx);
                if !shape.is_empty() && shape_clip_hits(&shape, from, to, Pos::new(cur_x, cur_y, cur_z)) {
                    return true;
                }
            }
            if norm_x > 1.0 && norm_y > 1.0 && norm_z > 1.0 {
                return false;
            }
            if norm_x < norm_y {
                if norm_x < norm_z {
                    cur_x += dx;
                    norm_x += norm_diff_x;
                } else {
                    cur_z += dz;
                    norm_z += norm_diff_z;
                }
            } else if norm_y < norm_z {
                cur_y += dy;
                norm_y += norm_diff_y;
            } else {
                cur_z += dz;
                norm_z += norm_diff_z;
            }
        }
    }

    // getSeenFraction: the share of rays from a box's points to the
    // explosion's center that no block stops.
    fn seen_fraction(&mut self, l: &mut Level, ctx: Ctx, source: Vec3, bb: &Aabb) -> f32 {
        let (diff_x, diff_y, diff_z) = (bb.max_x - bb.min_x, bb.max_y - bb.min_y, bb.max_z - bb.min_z);
        let inc_x = 1.0 / (diff_x * 2.0 + 1.0);
        let inc_y = 1.0 / (diff_y * 2.0 + 1.0);
        let inc_z = 1.0 / (diff_z * 2.0 + 1.0);
        if inc_x < 0.0 || inc_y < 0.0 || inc_z < 0.0 {
            return 0.0f32;
        }
        let off_x = (1.0 - (1.0 / inc_x).floor() * inc_x) * 0.5 + bb.min_x;
        let off_y = bb.min_y;
        let off_z = (1.0 - (1.0 / inc_z).floor() * inc_z) * 0.5 + bb.min_z;
        let mut total = 0i32;
        let mut missed = 0i32;
        let mut dx = 0.0f64;
        while dx <= 1.0 {
            let from_x = dx.mul_add(diff_x, off_x);
            let mut dy = 0.0f64;
            while dy <= 1.0 {
                let from_y = dy.mul_add(diff_y, off_y);
                let mut dz = 0.0f64;
                while dz <= 1.0 {
                    total += 1;
                    let from = Vec3::new(from_x, from_y, dz.mul_add(diff_z, off_z));
                    if !self.clips_anything(l, ctx, from, source) {
                        missed += 1;
                    }
                    dz += inc_z;
                }
                dy += inc_y;
            }
            dx += inc_x;
        }
        missed as f32 / total as f32
    }
}

// Math.signum(double).
fn signum(d: f64) -> f64 {
    if d > 0.0 {
        1.0
    } else if d < 0.0 {
        -1.0
    } else {
        d
    }
}

// An entity's share of the damage and knockback: ExplosionDamageCalculator.getEntityDamageAmount.
fn damage_amount(radius: f32, center: Vec3, e: &Entity, seen: f32) -> f32 {
    let f = radius * 2.0f32;
    let d = distance_sqr(e.pos, center).sqrt() / f as f64;
    let d1 = (1.0 - d) * seen as f64;
    ((d1 * d1 + d1) / 2.0 * 7.0 * f as f64 + 1.0) as f32
}

// Entity.distanceToSqr(Vec3): this - other.
fn distance_sqr(pos: Vec3, other: Vec3) -> f64 {
    let d = pos.x - other.x;
    let d1 = pos.y - other.y;
    let d2 = pos.z - other.z;
    d * d + d1 * d1 + d2 * d2
}

// Level.explode of a primed TNT: ServerExplosion.explode with the damage
// calculator of its source entity.
pub fn explode(l: &mut Level, source: &Entity, center: Vec3, radius: f32, fire: bool) {
    if fire {
        refuse("an explosion that makes fire");
    }
    let primed_cart = matches!(&source.kind, Kind::Minecart(c) if c.class == entity::minecart::Class::Tnt && c.fuse > -1);
    let mut explosion = Explosion { center, radius, fire, primed_cart, cache: HashMap::new() };
    // Level.gameEvent(source, EXPLODE, center), before the blocks are found.
    let event = crate::game_event!(l, "explode");
    let source_context = crate::game_event::Context::of_entity(Some(crate::game_event::Source::of(l, source)));
    l.game_event(event, center, source_context);
    let mut positions = explosion.exploded_positions(l);
    hurt_entities(l, &mut explosion, source);
    interact_with_blocks(l, &mut positions);
}

// ServerExplosion.hurtEntities.
fn hurt_entities(l: &mut Level, explosion: &mut Explosion, source: &Entity) {
    let center = explosion.center;
    let f = explosion.radius * 2.0f32;
    let floor = phys::floor(center.x - f as f64 - 1.0);
    let floor1 = phys::floor(center.x + f as f64 + 1.0);
    let floor2 = phys::floor(center.y - f as f64 - 1.0);
    let floor3 = phys::floor(center.y + f as f64 + 1.0);
    let floor4 = phys::floor(center.z - f as f64 - 1.0);
    let floor5 = phys::floor(center.z + f as f64 + 1.0);
    let area = Aabb::new(floor as f64, floor2 as f64, floor4 as f64, floor1 as f64, floor3 as f64, floor5 as f64);
    // The explosion's source is not hurt (excludeSourceFromDamage); the
    // predicate asks for entities alive and not spectating.
    let list = entities_in(l, Some(source), Some(source.id), crate::entity::Only::Any, &area, |e| entity::is_alive(e));
    for id in list {
        let (pos, eye_pos, bb, is_tnt, is_frame) = {
            let e = l.entities.all.get(&id).expect("an entity in the explosion's box");
            (e.pos, Vec3::new(e.pos.x, e.eye_y(), e.pos.z), e.bb, matches!(e.kind, Kind::Tnt(_)), matches!(e.kind, Kind::Frame(_) | Kind::Painting(_)))
        };
        // BlockAttachedEntity.ignoreExplosion: a source in water leaves it be.
        if is_frame && crate::entity::frame::ignores_explosion(source) {
            continue;
        }
        // ArmorStand.ignoreExplosion: an invisible stand.
        if matches!(l.entities.all[&id].kind, Kind::ArmorStand(_)) && crate::entity::armor_stand::ignores_explosion(&l.entities.all[&id]) {
            continue;
        }
        let mut d = distance_sqr(pos, center).sqrt() / f as f64;
        if d.is_nan() {
            d = f64::INFINITY;
        }
        if d > 1.0 {
            continue;
        }
        let vec3 = if is_tnt { pos } else { eye_pos };
        let direction = vec3.subtract(center).normalize();
        // shouldDamageEntity true, knockback multiplier 1.0.
        let ctx = crate::entity::collide::context(l, &l.entities.all[&id]);
        let density = explosion.seen_fraction(l, ctx, center, &bb);
        // Damage: the entity's hurtServer.
        let amount = {
            let e = l.entities.all.get(&id).unwrap();
            damage_amount(explosion.radius, center, e, density)
        };
        hurt(l, id, amount);
        // Knockback.
        let d2 = (1.0 - d) * density as f64 * 1.0f32 as f64 * (1.0 - 0.0);
        let push = direction.scale(d2);
        // A destroyed item is pushed too, to no effect; an item frame is not
        // pushed (BlockAttachedEntity.push does nothing).
        if !is_frame {
            if let Some(e) = l.entities.all.get_mut(&id) {
                e.delta = e.delta.add_vec(push);
                e.has_impulse = true;
            }
        }
    }
}

// Entity.hurtServer of an explosion's damage: an item entity loses health
// and may be destroyed, a frame breaks, a minecart takes damage; a falling
// block or primed TNT takes none.
fn hurt(l: &mut Level, id: EntityId, amount: f32) {
    if matches!(l.entities.all[&id].kind, Kind::Falling(_) | Kind::Tnt(_)) {
        return;
    }
    let mut e = l.entities.all.remove(&id).unwrap();
    match e.kind {
        Kind::Frame(_) => crate::entity::frame::hurt_by_explosion(l, &mut e),
        Kind::Painting(_) => crate::entity::painting::hurt_by_explosion(l, &mut e),
        Kind::ArmorStand(_) => {
            crate::entity::armor_stand::hurt(l, &mut e, crate::entity::damage::Dmg::Explosion, amount);
        }
        Kind::Item(_) => item::hurt_by_explosion(l, &mut e, amount),
        Kind::Minecart(_) | Kind::Boat(_) => {
            crate::entity::vehicle::hurt(l, &mut e, crate::entity::damage::Dmg::Explosion, amount);
        }
        Kind::Orb(_) => {
            crate::entity::orb::hurt(l, &mut e, crate::entity::damage::Dmg::Explosion, amount);
        }
        Kind::Golem(_) => crate::entity::golem::unported(&e, "is hurt by an explosion"),
        Kind::Falling(_) | Kind::Tnt(_) => unreachable!(),
    }
    if !e.removed {
        l.entities.all.insert(id, e);
    }
}

// ServerExplosion.StackCollector.
struct Collector {
    pos: Pos,
    stack: ItemStack,
}

// ServerExplosion.interactWithBlocks, then the stacks it collected popped out.
fn interact_with_blocks(l: &mut Level, blocks: &mut Vec<Pos>) {
    // Util.shuffle(blocks, level.random).
    let size = blocks.len();
    for i in (2..=size).rev() {
        let j = l.random.next_int_bound(i as i32) as usize;
        blocks.swap(i - 1, j);
    }
    // The block list EntityExplodeEvent gets: the blocks in reverse, without
    // air; the explosion goes on with it.
    let mut ordered = Vec::new();
    for i in (0..blocks.len()).rev() {
        let state = l.get(blocks[i]);
        if !l.is_air(state) {
            ordered.push(blocks[i]);
        }
    }
    let mut collectors: Vec<Collector> = Vec::new();
    for pos in ordered {
        let state = l.get(pos);
        // TNT blocks: callTNTPrimeEvent (no listener).
        on_explosion_hit(l, state, pos, &mut collectors);
    }
    for collector in collectors {
        item::pop_resource(l, collector.pos, collector.stack);
    }
}

// BlockBehaviour.onExplosionHit with the explosion's block interaction DESTROY
// (it cannot trigger blocks, so the overrides that act on it only call the
// default; the beehive's then angers the bees near it, and there are none),
// and TntBlock's dropFromExplosion and wasExploded. A moving piston drops what
// its block entity moves (MovingPistonBlock.getDrops, item::get_drops), and
// setting it to air ends its move (PistonMovingBlockEntity.finalTick).
fn on_explosion_hit(l: &mut Level, state: State, pos: Pos, collectors: &mut Vec<Collector>) {
    let class = l.reg.blocks[l.block(state) as usize].class.clone();
    if crate::entity::motion::overrides(l, state, "onExplosionHit")
        && !matches!(
            class.as_str(),
            "ButtonBlock"
                | "DoorBlock"
                | "WeatheringCopperDoorBlock"
                | "FenceGateBlock"
                | "LeverBlock"
                | "TrapDoorBlock"
                | "WeatheringCopperTrapDoorBlock"
                | "CandleBlock"
                | "CandleCakeBlock"
                | "BellBlock"
                | "BeehiveBlock"
                | "CreakingHeartBlock"
        )
    {
        refuse(format!("{class}.onExplosionHit ({})", l.reg.describe(state)));
    }
    // CreakingHeartBlock.onExplosionHit: the heart's block entity
    // (Level.getBlockEntity makes it if missing) loses its creaking, which it
    // never has here (saved data naming one is refused); the explosion's
    // indirect source is no player, so no experience. Then the default.
    if class == "CreakingHeartBlock" {
        l.block_entity(pos);
    }
    if l.is_air(state) || !is_destroyable(l, state) {
        return;
    }
    if class != "TntBlock" {
        // The block entity the loot context holds (made if missing).
        if l.has_block_entity(state) {
            l.block_entity(pos);
        }
        item::spawn_after_break_no_experience(l, state);
        // The loot params' THIS_ENTITY is the explosion's direct source.
        for stack in item::get_drops(l, state, pos, true) {
            add_or_append_stack(l, collectors, stack, pos);
        }
    }
    l.set(pos, l.known.air, UPDATE_ALL);
    if class == "TntBlock" {
        entity::tnt::was_exploded(l, pos);
    }
}

// ServerExplosion.addOrAppendStack: the stack joins the first collected stack
// it merges with (ItemEntity.areMergable, merge up to 16). An empty stack
// stops at the first collector (it is empty after tryMerge) or, with none,
// is collected itself, and pops out as nothing.
fn add_or_append_stack(l: &Level, collectors: &mut Vec<Collector>, mut stack: ItemStack, pos: Pos) {
    for collector in collectors.iter_mut() {
        // ItemEntity.areMergable(collector's, stack): the sizes add up to the
        // stack's maximum, the same item and components.
        if stack.count + collector.stack.count <= stack.max_stack_size(l.reg) && crate::blocks::container::same_item_same_components(&collector.stack, &stack) {
            let moved = (collector.stack.max_stack_size(l.reg).min(16) - collector.stack.count).min(stack.count);
            collector.stack.count += moved;
            stack.count -= moved;
        }
        if stack.count <= 0 {
            return;
        }
    }
    collectors.push(Collector { pos, stack });
}
