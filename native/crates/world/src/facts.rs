//! What the import and the region reader ask of Minecraft, from the engine's
//! table (`core/minecraft/<version>/blocks.txt`, `McbenchEngine.dump`), as
//! `server/minecraft-facts.js` reads it: the blocks, their properties and
//! states, whether a block is air, and what each state placed alone does
//! (`alone`), and the rest of `minecraft-facts.js` (`mcbench-setup` added
//! it): each block's class and supertypes, the item tags, the directions in
//! which a block that cannot survive alone is held, and every block with its
//! properties in the table's order (the block registry a build program is
//! checked against).

use std::collections::HashMap;

use crate::{BlockState, Palette};

/// One block of the table.
#[derive(Debug)]
struct Block {
    class_name: String,
    supertypes: Vec<String>,
    properties: Vec<(String, Vec<String>)>,
    states: HashMap<String, u32>,
    defaults: Vec<usize>,
    air: bool,
}

/// Direction.getUnitVec3i, in Direction.ordinal() order (down, up, north,
/// south, west, east).
const STEPS: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];

/// One block of the table as [`MinecraftFacts::blocks`] lists it: its id,
/// and each property's name, values and default value.
pub type BlockProperties<'a> = (&'a str, Vec<(&'a str, &'a [String], &'a str)>);

/// The table's facts (see the module's doc).
#[derive(Debug)]
pub struct MinecraftFacts {
    blocks: HashMap<String, Block>,
    order: Vec<String>,
    item_tags: HashMap<String, Vec<String>>,
    alone: Vec<u8>,
    /// By state, 1 when it survives alone (canSurvive in an empty level).
    pub survives: Vec<u8>,
    /// By state that does not survive alone, the directions (bits over
    /// Direction.ordinal()) in which a block of stone alone lets it survive.
    pub held: Vec<u8>,
}

/// What the import needs to order its passes (`minecraftFacts().lastPass`):
/// for each palette entry (entry 0 air), 1 when its state would not stay as
/// placed alone. [`MinecraftFacts`] answers from Minecraft's table; tests
/// answer with a stand-in, as today's `test/helpers/fake-facts.js`.
pub trait LastPass {
    /// By palette index, 1 for the import's last pass.
    fn last_pass(&self, palette: &Palette) -> Result<Vec<u8>, String>;
}

impl MinecraftFacts {
    /// Reads the table's text.
    pub fn new(text: &str) -> Result<MinecraftFacts, String> {
        let mut blocks: HashMap<String, Block> = HashMap::new();
        let mut order = Vec::new();
        let mut item_tags = HashMap::new();
        let mut current: Option<(String, u32)> = None;
        let mut alone = None;
        let mut held_line = None;
        for line in text.split('\n') {
            let Some(tab) = line.find('\t') else { continue };
            let fields: Vec<&str> = line.split('\t').collect();
            match &line[..tab] {
                "block" => {
                    let key = fields[1].to_string();
                    let default_state: u32 = fields[6]
                        .parse()
                        .map_err(|_| format!("Block {key} has default state {:?}", fields[6]))?;
                    let class = fields[2];
                    let supertypes = fields
                        .get(11)
                        .ok_or_else(|| format!("Block {key} has no supertypes"))?;
                    order.push(key.clone());
                    blocks.insert(
                        key.clone(),
                        Block {
                            class_name: class[class.rfind('.').map_or(0, |i| i + 1)..].to_string(),
                            supertypes: if *supertypes == "-" {
                                Vec::new()
                            } else {
                                supertypes.split(',').map(str::to_string).collect()
                            },
                            properties: Vec::new(),
                            states: HashMap::new(),
                            defaults: Vec::new(),
                            air: false,
                        },
                    );
                    current = Some((key, default_state));
                }
                "property" => {
                    let (key, _) = current.as_ref().ok_or("A property line before any block")?;
                    let block = blocks.get_mut(key).expect("made above");
                    block.properties.push((
                        fields[1].to_string(),
                        fields[2].split(',').map(str::to_string).collect(),
                    ));
                }
                "state" => {
                    let (key, default_state) =
                        current.as_ref().ok_or("A state line before any block")?;
                    let block = blocks.get_mut(key).expect("made above");
                    let id: u32 = fields[1]
                        .parse()
                        .map_err(|_| format!("State {:?} of {key} is not a number", fields[1]))?;
                    block.states.insert(fields[2].to_string(), id);
                    if id == *default_state {
                        block.defaults = if fields[2] == "-" {
                            Vec::new()
                        } else {
                            fields[2]
                                .split(',')
                                .map(|n| {
                                    n.parse().map_err(|_| {
                                        format!("State {id} of {key} has indices {:?}", fields[2])
                                    })
                                })
                                .collect::<Result<_, String>>()?
                        };
                        // Flag 1: BlockState.isAir. A line without flags is
                        // not air (`Number(undefined) & 1` is 0).
                        let flags: u32 = match fields.get(3) {
                            Some(text) => text
                                .parse()
                                .map_err(|_| format!("State {id} of {key} has flags {text:?}"))?,
                            None => 0,
                        };
                        block.air = flags & 1 == 1;
                    }
                }
                "alone" => {
                    alone = Some(
                        line[tab + 1..]
                            .split(',')
                            .map(|bits| {
                                u8::from_str_radix(bits, 16)
                                    .map_err(|_| format!("The alone line holds {bits:?}"))
                            })
                            .collect::<Result<Vec<u8>, String>>()?,
                    );
                }
                "itemtag" => {
                    let members = fields
                        .get(2)
                        .ok_or_else(|| format!("The item tag line {line:?} is short"))?;
                    item_tags.insert(
                        fields[1].to_string(),
                        if *members == "-" {
                            Vec::new()
                        } else {
                            members.split(',').map(str::to_string).collect()
                        },
                    );
                }
                "held" => held_line = Some(line[tab + 1..].to_string()),
                _ => {}
            }
        }
        let alone = alone.ok_or("The engine's table has no alone line")?;
        let mut survives = vec![1u8; alone.len()];
        let mut held = vec![0u8; alone.len()];
        for entry in held_line
            .ok_or("The engine's table has no held line")?
            .split(',')
            .filter(|e| !e.is_empty())
        {
            let bad = || format!("The held line holds {entry:?}");
            let (state, directions) = entry.split_once(':').ok_or_else(bad)?;
            let state: usize = state.parse().map_err(|_| bad())?;
            *survives.get_mut(state).ok_or_else(bad)? = 0;
            held[state] = directions.parse().map_err(|_| bad())?;
        }
        Ok(MinecraftFacts {
            blocks,
            order,
            item_tags,
            alone,
            survives,
            held,
        })
    }

    /// How many states.
    pub fn count(&self) -> usize {
        self.alone.len()
    }

    /// The simple name of the class of block `id` (`BedBlock`).
    pub fn class_name(&self, id: &str) -> Result<&str, String> {
        Ok(&self.block(id)?.class_name)
    }

    /// Whether block `id` is of Minecraft's class `class_name`: its own
    /// class or a superclass below Block (the dump's supertypes).
    pub fn is(&self, id: &str, class_name: &str) -> Result<bool, String> {
        let block = self.block(id)?;
        Ok(block.class_name == class_name || block.supertypes.iter().any(|s| s == class_name))
    }

    /// The items of the item tag `name`, in the table's order.
    pub fn item_tag(&self, name: &str) -> Result<&[String], String> {
        self.item_tags
            .get(name)
            .map(Vec::as_slice)
            .ok_or_else(|| format!("Minecraft has no item tag {name}"))
    }

    /// The properties of block `id`'s default state, by name in the
    /// table's order.
    pub fn defaults(&self, id: &str) -> Result<Vec<(String, String)>, String> {
        let block = self.block(id)?;
        Ok(block
            .properties
            .iter()
            .zip(&block.defaults)
            .map(|((name, values), &i)| (name.clone(), values[i].clone()))
            .collect())
    }

    /// Where a full block, alone beside block `id` with `properties`, lets
    /// it survive when it cannot alone: the offsets of those neighbours in
    /// Direction order (`heldBy`).
    pub fn held_by(
        &self,
        id: &str,
        properties: &[(String, String)],
    ) -> Result<Vec<[i32; 3]>, String> {
        // `this.held[undefined]` is undefined, whose bits are all 0, where
        // today's lookup finds no state.
        let directions = self
            .state_id(id, properties)?
            .and_then(|s| self.held.get(s as usize).copied())
            .unwrap_or(0);
        Ok(STEPS
            .iter()
            .enumerate()
            .filter(|(d, _)| (directions >> d) & 1 == 1)
            .map(|(_, step)| *step)
            .collect())
    }

    /// Every block in the table's order.
    pub fn blocks(&self) -> impl Iterator<Item = BlockProperties<'_>> {
        self.order.iter().map(|id| {
            let block = &self.blocks[id];
            let properties = block
                .properties
                .iter()
                .zip(&block.defaults)
                .map(|((name, values), &i)| (name.as_str(), values.as_slice(), values[i].as_str()))
                .collect();
            (id.as_str(), properties)
        })
    }

    fn block(&self, id: &str) -> Result<&Block, String> {
        self.blocks
            .get(id)
            .ok_or_else(|| format!("Minecraft has no block {id}"))
    }

    /// The state id of block `id` with `properties`, its block's defaults
    /// where it names none (`stateId`); `None` where today's lookup finds no
    /// state.
    pub fn state_id(
        &self,
        id: &str,
        properties: &[(String, String)],
    ) -> Result<Option<u32>, String> {
        let block = self.block(id)?;
        let mut indices = block.defaults.clone();
        for (name, value) in properties {
            let at = block
                .properties
                .iter()
                .position(|(property, _)| property == name);
            let index = at.and_then(|at| block.properties[at].1.iter().position(|v| v == value));
            let (Some(at), Some(index)) = (at, index) else {
                return Err(format!("Minecraft has no block state {id}[{name}={value}]"));
            };
            indices[at] = index;
        }
        let key = if indices.is_empty() {
            "-".to_string()
        } else {
            indices
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        Ok(block.states.get(&key).copied())
    }

    /// Whether block `id` is air to Minecraft (air, cave air, void air:
    /// `isAir`).
    pub fn is_air(&self, id: &str) -> Result<bool, String> {
        Ok(self.block(id)?.air)
    }

    /// Whether state `state` would not stay as placed alone.
    fn alone_bits(&self, entry: &BlockState) -> Result<u8, String> {
        // `this.alone[undefined]` is undefined, which is falsy, where today's
        // lookup finds no state.
        Ok(match self.state_id(&entry.id, &entry.properties)? {
            Some(state) => self.alone.get(state as usize).copied().unwrap_or(0),
            None => 0,
        })
    }
}

impl LastPass for MinecraftFacts {
    fn last_pass(&self, palette: &Palette) -> Result<Vec<u8>, String> {
        let mut last = vec![0u8; palette.len()];
        for (index, slot) in last.iter_mut().enumerate().skip(1) {
            *slot = u8::from(self.alone_bits(palette.entry(index))? != 0);
        }
        Ok(last)
    }
}
