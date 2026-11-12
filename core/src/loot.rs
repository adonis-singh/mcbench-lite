// Block loot: the loot tables (core/minecraft/<version>/loot.json, as the
// server's reloadable registries hold them), evaluated as LootTable,
// LootPool, the entries, conditions, functions and number providers do for
// a block broken with no tool and no entity, with no explosion; and the
// random sequences they draw from (RandomSequences, XoroshiroRandomSource,
// RandomSupport). A part of a table this does not model refuses when the
// evaluation reaches it.

use crate::json::Json;
use crate::level::refuse;
use crate::registry::{BlockId, Registry, State};
use std::collections::HashMap;
use std::sync::OnceLock;

// --- Random sequences ---

fn mix_stafford13(mut seed: u64) -> u64 {
    seed = (seed ^ (seed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    seed = (seed ^ (seed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    seed ^ (seed >> 31)
}

const GOLDEN_RATIO_64: u64 = 0x9E37_79B9_7F4A_7C15;
const SILVER_RATIO_64: u64 = 0x6A09_E667_F3BC_C909;

// Xoroshiro128PlusPlus as XoroshiroRandomSource uses it.
#[derive(Clone)]
pub struct Xoroshiro {
    lo: u64,
    hi: u64,
}

impl Xoroshiro {
    fn new(lo: u64, hi: u64) -> Xoroshiro {
        if lo | hi == 0 {
            Xoroshiro { lo: GOLDEN_RATIO_64, hi: SILVER_RATIO_64 }
        } else {
            Xoroshiro { lo, hi }
        }
    }

    pub fn next_long(&mut self) -> i64 {
        let (l, mut l1) = (self.lo, self.hi);
        let result = l.wrapping_add(l1).rotate_left(17).wrapping_add(l);
        l1 ^= l;
        self.lo = l.rotate_left(49) ^ l1 ^ (l1 << 21);
        self.hi = l1.rotate_left(28);
        result as i64
    }

    pub fn next_int(&mut self) -> i32 {
        self.next_long() as i32
    }

    pub fn next_int_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");
        let bound64 = bound as i64;
        let mut l = self.next_int() as u32 as i64;
        let mut l1 = l * bound64;
        let mut l2 = l1 & 0xFFFF_FFFF;
        if l2 < bound64 {
            let i = ((!bound).wrapping_add(1) as u32 % bound as u32) as i64;
            while l2 < i {
                l = self.next_int() as u32 as i64;
                l1 = l * bound64;
                l2 = l1 & 0xFFFF_FFFF;
            }
        }
        (l1 >> 32) as i32
    }

    fn next_bits(&mut self, bits: u32) -> u64 {
        (self.next_long() as u64) >> (64 - bits)
    }

    pub fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 * 5.9604645E-8f32
    }

    pub fn next_double(&mut self) -> f64 {
        (self.next_bits(53) as f32 * 1.110223E-16f32) as f64
    }
}

// The 128 bits of MD5(text), as two big-endian longs (RandomSupport.seedFromHashOf).
fn md5(message: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11,
        16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut data = message.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&((message.len() as u64).wrapping_mul(8)).to_le_bytes());
    for chunk in data.chunks(64) {
        let m: Vec<u32> = (0..16).map(|i| u32::from_le_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap())).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (mut f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, v) in [a0, b0, c0, d0].iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

// RandomSequences of a new world: each sequence starts when first asked for.
pub struct RandomSequences {
    world_seed: i64,
    sequences: HashMap<String, Xoroshiro>,
}

impl RandomSequences {
    pub fn new(world_seed: i64) -> RandomSequences {
        RandomSequences { world_seed, sequences: HashMap::new() }
    }

    // Each started sequence's generator (low and high seed), by key.
    pub fn states(&self) -> Vec<(String, i64, i64)> {
        let mut out: Vec<(String, i64, i64)> = self.sequences.iter().map(|(key, r)| (key.clone(), r.lo as i64, r.hi as i64)).collect();
        out.sort();
        out
    }

    // RandomSequences.get: the sequence's random source.
    pub fn get(&mut self, location: &str) -> &mut Xoroshiro {
        let seed = self.world_seed;
        self.sequences.entry(location.to_string()).or_insert_with(|| {
            // createSequence(location, salt 0, includeWorldSeed, includeSequenceId): RandomSequence(seed, location).
            let l = seed as u64;
            let (lo, hi) = (l ^ SILVER_RATIO_64, (l ^ SILVER_RATIO_64).wrapping_add(GOLDEN_RATIO_64));
            let hash = md5(location.as_bytes());
            let key_lo = u64::from_be_bytes(hash[0..8].try_into().unwrap());
            let key_hi = u64::from_be_bytes(hash[8..16].try_into().unwrap());
            Xoroshiro::new(mix_stafford13(lo ^ key_lo), mix_stafford13(hi ^ key_hi))
        })
    }
}

// --- Item stacks ---

// An item stack: its item, its count and the patch to its item's components.
// Loot gives a stack none.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemStack {
    pub item: u16,
    pub count: i32,
    pub patch: crate::components::Patch,
}

// --- Tables ---

enum Number {
    Constant(f32),
    Uniform(Box<Number>, Box<Number>),
    Binomial(Box<Number>, Box<Number>),
}

enum Condition {
    SurvivesExplosion,
    BlockStateProperty { block: BlockId, properties: Vec<(String, String)> },
    AnyOf(Vec<Condition>),
    Inverted(Box<Condition>),
    RandomChance(Number),
    TableBonus(Vec<f32>),
    // A tool predicate that an empty tool never meets.
    ToolNeverMatches,
    // LootItemEntityPropertyCondition on the entity "this" with the empty
    // predicate: EntityPredicate.matches holds for any entity, and is false
    // for none (a block broken by no entity).
    ThisEntityPresent,
    // LocationCheck with a block predicate (one block, some state values) at
    // an offset from the broken block.
    LocationCheck { offset: [i32; 3], block: Option<BlockId>, state: Vec<(String, String)> },
    Unported(String),
}

enum Formula {
    Binomial { extra: i32, probability: f32 },
    OreDrops,
    Uniform { multiplier: i32 },
}

enum Function {
    SetCount { conditions: Vec<Condition>, count: Number, add: bool },
    ApplyBonus { conditions: Vec<Condition>, formula: Formula },
    ExplosionDecay { conditions: Vec<Condition> },
    LimitCount { conditions: Vec<Condition>, min: Option<Number>, max: Option<Number> },
    // copy_components from the block entity: the components named that its
    // collected components hold (custom name, and the contents of a container).
    CopyBlockEntityComponents { conditions: Vec<Condition>, include: Vec<String> },
    // CopyBlockState: the named block's properties (by name) that the broken
    // state has, copied into the item's block_state component.
    CopyState { conditions: Vec<Condition>, block: BlockId, properties: Vec<String> },
    Unported(String),
}

enum Entry {
    Item { item: u16, weight: i32, quality: i32, conditions: Vec<Condition>, functions: Vec<Function> },
    // DynamicLoot: the stacks the loot params give under the name (a
    // decorated pot's sherds), each through the entry's functions.
    Dynamic { name: String, weight: i32, quality: i32, conditions: Vec<Condition>, functions: Vec<Function> },
    Alternatives { children: Vec<Entry>, conditions: Vec<Condition> },
    Unported(String),
}

struct Pool {
    rolls: Number,
    bonus_rolls: Number,
    conditions: Vec<Condition>,
    functions: Vec<Function>,
    entries: Vec<Entry>,
}

pub struct Table {
    random_sequence: Option<String>,
    pools: Vec<Pool>,
    functions: Vec<Function>,
}

pub struct Tables {
    // Each block's table (Block.getLootTable): most blocks have the one of
    // their name, a wall torch the torch's.
    by_block: HashMap<BlockId, std::sync::Arc<Table>>,
}

fn strip(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

fn number(json: &Json) -> Number {
    match json {
        Json::Number(n) => Number::Constant(*n as f32),
        Json::Object(map) => match strip(map["type"].str()) {
            "constant" => Number::Constant(map["value"].num() as f32),
            "uniform" => Number::Uniform(Box::new(number(&map["min"])), Box::new(number(&map["max"]))),
            "binomial" => Number::Binomial(Box::new(number(&map["n"])), Box::new(number(&map["p"]))),
            other => panic!("unknown number provider {other}"),
        },
        other => panic!("bad number provider {other:?}"),
    }
}

fn conditions(reg: &Registry, json: Option<&Json>) -> Vec<Condition> {
    json.map_or(Vec::new(), |list| list.array().iter().map(|c| condition(reg, c)).collect())
}

fn condition(reg: &Registry, json: &Json) -> Condition {
    let kind = strip(json.get("condition").expect("a condition").str());
    match kind {
        "survives_explosion" => Condition::SurvivesExplosion,
        "block_state_property" => {
            let block = reg.block_key(json.get("block").unwrap().str());
            let properties = json.get("properties").map_or(Vec::new(), |p| {
                p.object()
                    .iter()
                    .map(|(name, value)| match value {
                        Json::String(value) => (name.clone(), value.clone()),
                        other => panic!("ranged state property matcher {other:?}"),
                    })
                    .collect()
            });
            Condition::BlockStateProperty { block, properties }
        }
        "any_of" => Condition::AnyOf(conditions(reg, json.get("terms"))),
        "inverted" => Condition::Inverted(Box::new(condition(reg, json.get("term").unwrap()))),
        "random_chance" => Condition::RandomChance(number(json.get("chance").unwrap())),
        "table_bonus" => Condition::TableBonus(json.get("chances").unwrap().array().iter().map(|c| c.num() as f32).collect()),
        "location_check" => {
            // LocationCheck(predicate, offset) with only a block predicate.
            let predicate = json.get("predicate").map(|p| p.object());
            let block_predicate = predicate.and_then(|p| p.get("block")).map(|b| b.object());
            let supported = predicate.is_some_and(|p| p.keys().all(|k| k == "block"))
                && block_predicate.is_some_and(|b| b.keys().all(|k| k == "blocks" || k == "state") && matches!(b.get("blocks"), Some(Json::String(name)) if !name.starts_with('#')))
                && json.object().keys().all(|k| matches!(k.as_str(), "condition" | "predicate" | "offsetX" | "offsetY" | "offsetZ"));
            if !supported {
                return Condition::Unported("location_check".into());
            }
            let block_predicate = block_predicate.unwrap();
            let offset = |name: &str| json.get(name).map_or(0, |v| v.num() as i32);
            Condition::LocationCheck {
                offset: [offset("offsetX"), offset("offsetY"), offset("offsetZ")],
                block: Some(reg.block_key(block_predicate["blocks"].str())),
                state: block_predicate.get("state").map_or(Vec::new(), |s| {
                    s.object()
                        .iter()
                        .map(|(name, value)| match value {
                            Json::String(value) => (name.clone(), value.clone()),
                            other => panic!("ranged state matcher {other:?}"),
                        })
                        .collect()
                }),
            }
        }
        "match_tool" => {
            // ItemPredicate.test(ItemStack.EMPTY): a predicate naming items
            // other than air fails for air; one requiring an enchantment
            // fails for a stack with none.
            let Some(predicate) = json.get("predicate").map(|p| p.object()) else { return Condition::Unported("match_tool without a predicate".into()) };
            if !predicate.keys().all(|k| k == "items" || k == "predicates") {
                return Condition::Unported("match_tool".into());
            }
            let items_fail = predicate.get("items").map(|items| match items {
                Json::String(name) => name != "minecraft:air",
                _ => false,
            });
            let enchantments_fail = predicate.get("predicates").map(|components| {
                let components = components.object();
                components.len() == 1
                    && components.get("minecraft:enchantments").is_some_and(|list| {
                        !list.array().is_empty() && list.array().iter().all(|e| e.get("levels").and_then(|l| l.get("min")).is_some_and(|min| min.num() >= 1.0))
                    })
            });
            if items_fail == Some(true) || enchantments_fail == Some(true) {
                Condition::ToolNeverMatches
            } else {
                Condition::Unported("match_tool".into())
            }
        }
        "entity_properties" if json.get("entity").is_some_and(|e| e.str() == "this") && json.get("predicate").is_some_and(|p| p.object().is_empty()) => Condition::ThisEntityPresent,
        other => Condition::Unported(other.to_string()),
    }
}

fn function(reg: &Registry, json: &Json) -> Function {
    let kind = strip(json.get("function").expect("a function").str());
    let conditions = conditions(reg, json.get("conditions"));
    match kind {
        "set_count" => Function::SetCount { conditions, count: number(json.get("count").unwrap()), add: json.get("add").is_some_and(|a| a.bool()) },
        "apply_bonus" => {
            let enchantment = json.get("enchantment").unwrap().str();
            assert_eq!(enchantment, "minecraft:fortune", "apply_bonus enchantment {enchantment}");
            let parameters = json.get("parameters");
            let formula = match strip(json.get("formula").unwrap().str()) {
                "ore_drops" => Formula::OreDrops,
                "uniform_bonus_count" => Formula::Uniform { multiplier: parameters.unwrap().get("bonusMultiplier").unwrap().num() as i32 },
                "binomial_with_bonus_count" => Formula::Binomial {
                    extra: parameters.unwrap().get("extra").unwrap().num() as i32,
                    probability: parameters.unwrap().get("probability").unwrap().num() as f32,
                },
                other => panic!("unknown bonus formula {other}"),
            };
            Function::ApplyBonus { conditions, formula }
        }
        "explosion_decay" => Function::ExplosionDecay { conditions },
        "copy_components" => {
            let source = json.get("source").map(|s| s.str().to_string()).unwrap_or_default();
            let include: Vec<&str> = json.get("include").map_or(Vec::new(), |list| list.array().iter().map(|c| c.str()).collect());
            // A block entity this port holds collects a custom name, the items
            // of a container and a banner's patterns (none): what else a table
            // names is a component none of them has.
            if source == "block_entity" && !include.is_empty() {
                Function::CopyBlockEntityComponents { conditions, include: include.iter().map(|c| c.to_string()).collect() }
            } else {
                Function::Unported(format!("copy_components {source} {}", include.join(",")))
            }
        }
        "copy_state" => Function::CopyState {
            conditions,
            block: reg.block_key(json.get("block").unwrap().str()),
            properties: json.get("properties").map_or(Vec::new(), |list| list.array().iter().map(|p| p.str().to_string()).collect()),
        },
        "limit_count" => {
            let limit = json.get("limit").unwrap();
            match limit {
                Json::Number(n) => Function::LimitCount { conditions, min: Some(Number::Constant(*n as f32)), max: Some(Number::Constant(*n as f32)) },
                _ => Function::LimitCount { conditions, min: limit.get("min").map(number), max: limit.get("max").map(number) },
            }
        }
        other => Function::Unported(other.to_string()),
    }
}

fn functions(reg: &Registry, json: Option<&Json>) -> Vec<Function> {
    json.map_or(Vec::new(), |list| list.array().iter().map(|f| function(reg, f)).collect())
}

fn entry(reg: &Registry, json: &Json) -> Entry {
    let kind = strip(json.get("type").expect("an entry type").str());
    let conditions = conditions(reg, json.get("conditions"));
    match kind {
        "item" => Entry::Item {
            item: *reg.item_by_key.get(json.get("name").unwrap().str()).unwrap_or_else(|| panic!("no item {}", json.get("name").unwrap().str())),
            weight: json.get("weight").map_or(1, |w| w.num() as i32),
            quality: json.get("quality").map_or(0, |q| q.num() as i32),
            conditions,
            functions: functions(reg, json.get("functions")),
        },
        "dynamic" => Entry::Dynamic {
            name: json.get("name").unwrap().str().to_string(),
            weight: json.get("weight").map_or(1, |w| w.num() as i32),
            quality: json.get("quality").map_or(0, |q| q.num() as i32),
            conditions,
            functions: functions(reg, json.get("functions")),
        },
        "alternatives" => Entry::Alternatives { children: json.get("children").map_or(Vec::new(), |c| c.array().iter().map(|e| entry(reg, e)).collect()), conditions },
        other => Entry::Unported(other.to_string()),
    }
}

impl Tables {
    // Each block's table is the one its registry entry names (Block.getLootTable).
    pub fn parse(reg: &Registry, text: &str) -> Tables {
        let json = Json::parse(text);
        let mut tables: HashMap<String, std::sync::Arc<Table>> = HashMap::new();
        for (key, table) in json.object() {
            let pools = table.get("pools").map_or(Vec::new(), |pools| {
                pools
                    .array()
                    .iter()
                    .map(|pool| Pool {
                        rolls: number(pool.get("rolls").unwrap()),
                        bonus_rolls: pool.get("bonus_rolls").map_or(Number::Constant(0.0), number),
                        conditions: conditions(reg, pool.get("conditions")),
                        functions: functions(reg, pool.get("functions")),
                        entries: pool.get("entries").unwrap().array().iter().map(|e| entry(reg, e)).collect(),
                    })
                    .collect()
            });
            tables.insert(
                key.clone(),
                std::sync::Arc::new(Table { random_sequence: table.get("random_sequence").map(|s| s.str().to_string()), pools, functions: functions(reg, table.get("functions")) }),
            );
        }
        let mut by_block = HashMap::new();
        for (id, block) in reg.blocks.iter().enumerate() {
            let Some(table) = &block.loot_table else { continue };
            // The table "minecraft:blocks/x" is the file "minecraft:x".
            let name = format!("minecraft:{}", table.strip_prefix("minecraft:blocks/").unwrap_or_else(|| panic!("loot table {table}")));
            let found = tables.get(&name).unwrap_or_else(|| panic!("no loot table {name}"));
            by_block.insert(id as BlockId, found.clone());
        }
        Tables { by_block }
    }
}

const LOOT: &str = include_str!("../minecraft/1.21.10/loot.json");

static TABLES: OnceLock<Tables> = OnceLock::new();

// The loot tables of the registry's blocks, parsed once.
pub fn tables(reg: &Registry) -> &'static Tables {
    TABLES.get_or_init(|| Tables::parse(reg, LOOT))
}

// --- Evaluation ---

// LootContext for a block broken with no tool, and with no entity or an
// explosion's source as THIS_ENTITY.
struct Context<'a> {
    reg: &'a Registry,
    random: &'a mut Xoroshiro,
    state: State,
    // The block state at an offset from the broken block, if that chunk is
    // loaded (LocationPredicate reads the level).
    at: &'a mut dyn FnMut(i32, i32, i32) -> Option<State>,
    // The block entity's collected components (BlockEntity.collectComponents),
    // if the block has one.
    block_entity: Option<&'a crate::nbt::Compound>,
    // The loot params' dynamic drops (LootParams.Builder.withDynamicDrop) by
    // name.
    dynamic: &'a [(&'a str, Vec<ItemStack>)],
    // Whether the params hold THIS_ENTITY (an explosion's direct source).
    this_entity: bool,
}

// Math.round(float).
fn round(f: f32) -> i32 {
    (f as f64 + 0.5).floor() as i32
}

impl Number {
    fn float(&self, cx: &mut Context) -> f32 {
        match self {
            Number::Constant(v) => *v,
            // Mth.nextFloat(random, min, max).
            Number::Uniform(min, max) => {
                let (min, max) = (min.float(cx), max.float(cx));
                if min >= max {
                    min
                } else {
                    cx.random.next_float() * (max - min) + min
                }
            }
            Number::Binomial(..) => self.int(cx) as f32,
        }
    }

    fn int(&self, cx: &mut Context) -> i32 {
        match self {
            Number::Constant(v) => round(*v),
            // Mth.nextInt(random, min, max).
            Number::Uniform(min, max) => {
                let (min, max) = (min.int(cx), max.int(cx));
                if min >= max {
                    min
                } else {
                    cx.random.next_int_bound(max - min + 1) + min
                }
            }
            Number::Binomial(n, p) => {
                let n = n.int(cx);
                let p = p.float(cx);
                let mut count = 0;
                for _ in 0..n {
                    if cx.random.next_float() < p {
                        count += 1;
                    }
                }
                count
            }
        }
    }
}

impl Condition {
    fn test(&self, cx: &mut Context) -> bool {
        match self {
            // No explosion radius: the block survives.
            Condition::SurvivesExplosion => true,
            Condition::BlockStateProperty { block, properties } => {
                cx.reg.block(cx.state) == *block
                    && properties.iter().all(|(name, value)| {
                        let b = &cx.reg.blocks[*block as usize];
                        b.properties.iter().find(|&&(id, _)| cx.reg.properties[id as usize].name == *name).is_some_and(|&(id, _)| cx.reg.value_name(cx.state, id) == value)
                    })
            }
            // Util.anyOf: the terms in order until one holds.
            Condition::AnyOf(terms) => terms.iter().any(|t| t.test(cx)),
            Condition::Inverted(term) => !term.test(cx),
            Condition::RandomChance(chance) => {
                let chance = chance.float(cx);
                cx.random.next_float() < chance
            }
            // The empty tool has no level of the enchantment.
            Condition::TableBonus(chances) => {
                let chance = chances[0.min(chances.len() - 1)];
                cx.random.next_float() < chance
            }
            Condition::ToolNeverMatches => false,
            Condition::ThisEntityPresent => cx.this_entity,
            // BlockPredicate.matches: a loaded chunk, the block, then each
            // named state value.
            Condition::LocationCheck { offset, block, state } => {
                let Some(found) = (cx.at)(offset[0], offset[1], offset[2]) else { return false };
                if let Some(block) = block {
                    if cx.reg.block(found) != *block {
                        return false;
                    }
                }
                state.iter().all(|(name, value)| {
                    let b = &cx.reg.blocks[cx.reg.block(found) as usize];
                    b.properties.iter().find(|&&(id, _)| cx.reg.properties[id as usize].name == *name).is_some_and(|&(id, _)| cx.reg.value_name(found, id) == value)
                })
            }
            Condition::Unported(what) => refuse(format!("loot condition {what}")),
        }
    }
}

fn all(conditions: &[Condition], cx: &mut Context) -> bool {
    conditions.iter().all(|c| c.test(cx))
}

impl Function {
    fn apply(&self, stack: &mut ItemStack, cx: &mut Context) {
        match self {
            Function::SetCount { conditions, count, add } => {
                if all(conditions, cx) {
                    let base = if *add { stack.count } else { 0 };
                    stack.count = base.wrapping_add(count.int(cx));
                }
            }
            // The tool is present (empty), with no enchantment level.
            Function::ApplyBonus { conditions, formula } => {
                if all(conditions, cx) {
                    stack.count = match formula {
                        Formula::Binomial { extra, probability } => {
                            let mut count = stack.count;
                            for _ in 0..*extra {
                                if cx.random.next_float() < *probability {
                                    count += 1;
                                }
                            }
                            count
                        }
                        Formula::OreDrops => stack.count,
                        Formula::Uniform { multiplier: _ } => stack.count + cx.random.next_int_bound(1),
                    };
                }
            }
            // No explosion radius: the count stays.
            Function::ExplosionDecay { conditions } => {
                all(conditions, cx);
            }
            Function::LimitCount { conditions, min, max } => {
                if all(conditions, cx) {
                    stack.count = match (min, max) {
                        (None, None) => stack.count,
                        (None, Some(max)) => max.int(cx).min(stack.count),
                        (Some(min), None) => min.int(cx).max(stack.count),
                        (Some(min), Some(max)) => {
                            // Mth.clamp(value, min, max) evaluates the bounds in order.
                            let (min, max) = (min.int(cx), max.int(cx));
                            stack.count.max(min).min(max)
                        }
                    };
                }
            }
            Function::CopyBlockEntityComponents { conditions, include } => {
                if all(conditions, cx) {
                    if let Some(components) = cx.block_entity {
                        for key in include {
                            if let Some(value) = components.get(key) {
                                stack.set_component(cx.reg, key, value.clone());
                            }
                        }
                    }
                }
            }
            // ItemStack.update(BLOCK_STATE, BlockItemStateProperties.EMPTY, ...):
            // each property's value put into the stack's map (its patch's or
            // its item's), then set, which a value equal to the item's own
            // leaves out of the patch. A property is the named block's object,
            // which another block's state has only if it is that block.
            Function::CopyState { conditions, block, properties } => {
                if all(conditions, cx) {
                    if cx.reg.block(cx.state) != *block {
                        refuse(format!("loot function copy_state of {} for {}", cx.reg.blocks[*block as usize].key, cx.reg.describe(cx.state)));
                    }
                    let mut map = match stack.component(cx.reg, "minecraft:block_state") {
                        Some(crate::nbt::Tag::Compound(map)) => map,
                        None => crate::nbt::Compound::new(),
                        Some(other) => panic!("a block_state component {other:?}"),
                    };
                    for name in properties {
                        let id = cx.reg.property_of(cx.state, name);
                        map.put(name, crate::nbt::Tag::string(cx.reg.value_name(cx.state, id)));
                    }
                    stack.set_component(cx.reg, "minecraft:block_state", crate::nbt::Tag::Compound(map));
                }
            }
            Function::Unported(what) => refuse(format!("loot function {what}")),
        }
    }
}

fn decorate(functions: &[Function], stack: &mut ItemStack, cx: &mut Context) {
    // LootItemFunctions.compose runs the functions in order.
    for f in functions {
        f.apply(stack, cx);
    }
}

// An entry that expanded: what createItemStack makes, an item or the
// dynamic drops of a name.
enum Made<'a> {
    Item(u16),
    Dynamic(&'a str),
}

struct Expanded<'a> {
    made: Made<'a>,
    weight: i32,
    functions: &'a [Function],
}

// LootPoolSingletonContainer.getWeight(luck 0): max(floor(weight + quality * 0), 0).
fn entry_weight(weight: i32, quality: i32) -> i32 {
    let w = (weight as f32 + quality as f32 * 0.0f32) as f64;
    w.floor().max(0.0) as i32
}

impl Entry {
    // expand: adds the entries it offers, and whether any did (what an
    // alternatives entry stops at).
    fn expand<'a>(&'a self, cx: &mut Context, out: &mut Vec<Expanded<'a>>) -> bool {
        match self {
            Entry::Item { item, weight, quality, conditions, functions } => {
                if all(conditions, cx) {
                    out.push(Expanded { made: Made::Item(*item), weight: entry_weight(*weight, *quality), functions });
                    true
                } else {
                    false
                }
            }
            Entry::Dynamic { name, weight, quality, conditions, functions } => {
                if all(conditions, cx) {
                    out.push(Expanded { made: Made::Dynamic(name), weight: entry_weight(*weight, *quality), functions });
                    true
                } else {
                    false
                }
            }
            Entry::Alternatives { children, conditions } => {
                if !all(conditions, cx) {
                    return false;
                }
                for child in children {
                    if child.expand(cx, out) {
                        return true;
                    }
                }
                false
            }
            Entry::Unported(what) => refuse(format!("loot entry {what}")),
        }
    }
}

impl Pool {
    fn add_random_item(&self, table_functions: &[Function], cx: &mut Context, output: &mut Vec<ItemStack>) {
        let mut offered = Vec::new();
        for entry in &self.entries {
            entry.expand(cx, &mut offered);
        }
        // Only entries with weight above 0 count.
        offered.retain(|e| e.weight > 0);
        let total: i32 = offered.iter().map(|e| e.weight).sum();
        if total == 0 || offered.is_empty() {
            return;
        }
        let chosen = if offered.len() == 1 {
            &offered[0]
        } else {
            let mut roll = cx.random.next_int_bound(total);
            let mut found = None;
            for e in &offered {
                roll -= e.weight;
                if roll < 0 {
                    found = Some(e);
                    break;
                }
            }
            match found {
                Some(e) => e,
                None => return,
            }
        };
        let made = match chosen.made {
            Made::Item(item) => vec![ItemStack::new(item, 1)],
            // LootContext.addDynamicDrops: none for a name the params lack.
            Made::Dynamic(name) => cx.dynamic.iter().find(|(n, _)| *n == name).map_or(Vec::new(), |(_, stacks)| stacks.clone()),
        };
        for mut stack in made {
            decorate(chosen.functions, &mut stack, cx);
            // The pool's functions wrap the entry's, the table's the pool's.
            decorate(&self.functions, &mut stack, cx);
            decorate(table_functions, &mut stack, cx);
            output.push(stack);
        }
    }

    fn add_random_items(&self, table_functions: &[Function], cx: &mut Context, output: &mut Vec<ItemStack>) {
        if all(&self.conditions, cx) {
            let rolls = self.rolls.int(cx) + (self.bonus_rolls.float(cx) * 0.0f32).floor() as i32;
            for _ in 0..rolls {
                self.add_random_item(table_functions, cx, output);
            }
        }
    }
}

// BlockBehaviour.getDrops for a block state broken with no tool or
// explosion radius, and with an entity as THIS_ENTITY or none: the block's loot table rolled on its random sequence, its
// stacks split at the item's stack size (Paper's splitOverstackedLoot).
// A block with no table drops nothing.
pub fn block_drops(
    reg: &Registry,
    sequences: &mut RandomSequences,
    state: State,
    block_entity: Option<&crate::nbt::Compound>,
    dynamic: &[(&str, Vec<ItemStack>)],
    this_entity: bool,
    at: &mut dyn FnMut(i32, i32, i32) -> Option<State>,
) -> Vec<ItemStack> {
    let block = reg.block(state);
    let Some(table) = tables(reg).by_block.get(&block) else { return Vec::new() };
    let sequence = table.random_sequence.as_deref().expect("a block loot table without a random sequence");
    let random = sequences.get(sequence);
    let mut cx = Context { reg, random, state, block_entity, dynamic, this_entity, at };
    let mut stacks = Vec::new();
    for pool in &table.pools {
        pool.add_random_items(&table.functions, &mut cx, &mut stacks);
    }
    let mut out = Vec::new();
    for stack in stacks {
        let max = stack.max_stack_size(reg);
        if stack.count >= max {
            let mut count = stack.count;
            while count > 0 {
                let part = max.min(count);
                count -= part;
                let mut piece = stack.clone();
                piece.count = part;
                out.push(piece);
            }
        } else {
            out.push(stack);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Block drops against Java's, for every block state three times each, from a
    // file of "<state id>\t<repeat>\t<item count;...>" lines (LOOT_ORACLE,
    // written by Block.getDrops on a seeded level). A block whose table has a
    // part this does not model refuses, and is not compared.
    #[test]
    fn drops_against_java() {
        let Ok(path) = std::env::var("LOOT_ORACLE") else { return };
        let reg = Registry::parse(include_str!("../minecraft/1.21.10/blocks.txt"));
        let text = std::fs::read_to_string(path).unwrap();
        let mut sequences = RandomSequences::new(0);
        let mut skipped = std::collections::HashSet::new();
        let (mut compared, mut refused) = (0, 0);
        for line in text.lines() {
            let p: Vec<&str> = line.split('\t').collect();
            let state: State = p[0].parse().unwrap();
            let block = reg.block(state);
            if skipped.contains(&block) {
                continue;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                block_drops(&reg, &mut sequences, state, None, &[], false, &mut |_, _, _| None)
            }));
            match result {
                Ok(drops) => {
                    // ItemStack.getItem of an empty stack is air.
                    let got: String = drops.iter().map(|s| format!("{} {};", if s.count <= 0 { "minecraft:air" } else { &reg.items[s.item as usize].key }, s.count)).collect();
                    assert_eq!(got, p[2], "drops of {} (repeat {})", reg.describe(state), p[1]);
                    compared += 1;
                }
                Err(payload) => {
                    assert!(payload.downcast_ref::<crate::level::Refusal>().is_some(), "a panic that is no refusal for {}", reg.describe(state));
                    skipped.insert(block);
                    refused += 1;
                    let reason = &payload.downcast_ref::<crate::level::Refusal>().unwrap().0;
                    println!("refused {} {reason}", reg.blocks[block as usize].key);
                }
            }
        }
        println!("compared {compared} drops, refused {refused} blocks");
        assert!(compared > 10000);
    }

    #[test]
    fn md5_known() {
        let hex = |d: [u8; 16]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex(md5(b"The quick brown fox jumps over the lazy dog")), "9e107d9d372bb6826bd81d3542a419d6");
    }
}
