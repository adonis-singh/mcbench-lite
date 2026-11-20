//! The colours Minecraft's client tints each block state with in the world
//! the app builds in (`server/block-tints.js`; the shipped `tints.tsv`,
//! `McbenchClient.java`), by state id (the block table's order, line for
//! line), each 0xRRGGBB: `world` by state and tint index, the colour of its
//! faces of that tint index; `particle` the colour its breaking particles,
//! which a block's icon shows, are multiplied by.

use mcbench_world::block_states::BlockStates;

/// The tints (see the module's doc).
#[derive(Debug)]
pub struct BlockTints {
    /// Tint indices per state.
    pub indices: usize,
    /// By state and tint index.
    pub world: Vec<u32>,
    /// By state.
    pub particle: Vec<u32>,
}

impl BlockTints {
    /// Reads the tints file `text` for the block table's `states`.
    pub fn new(text: &str, states: &BlockStates) -> Result<BlockTints, String> {
        let mut lines = text.split('\n').filter(|l| !l.is_empty());
        let head = lines.next().ok_or("The tints file is empty")?;
        let indices: usize = head
            .split('\t')
            .nth(1)
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| format!("The tints file begins {head:?}"))?;
        let lines: Vec<&str> = lines.collect();
        if lines.len() != states.count {
            return Err(format!(
                "The tints have {} states, the block table {}",
                lines.len(),
                states.count
            ));
        }
        let mut world = vec![0u32; states.count * indices];
        let mut particle = vec![0u32; states.count];
        let hex = |text: &str| {
            u32::from_str_radix(text, 16).map_err(|_| format!("The tints hold a colour {text:?}"))
        };
        for (id, line) in lines.iter().enumerate() {
            let fields: Vec<&str> = line.split('\t').collect();
            let [name, colours, _, particle_colour, ..] = fields[..] else {
                return Err(format!("The tints' line {line:?} is short"));
            };
            if states.ids.get(name).map(|&s| s as usize) != Some(id) {
                return Err(format!(
                    "The tints name state {id} {name}, which the block table does not"
                ));
            }
            for (index, colour) in colours.split(',').enumerate() {
                world[id * indices + index] = hex(colour)?;
            }
            particle[id] = hex(particle_colour)?;
        }
        Ok(BlockTints {
            indices,
            world,
            particle,
        })
    }

    /// What the renderer reads: u32 states, u32 tint indices, then by
    /// state and tint index u32 0xRRGGBB, little-endian.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.world.len() * 4);
        out.extend_from_slice(&((self.world.len() / self.indices) as u32).to_le_bytes());
        out.extend_from_slice(&(self.indices as u32).to_le_bytes());
        for colour in &self.world {
            out.extend_from_slice(&colour.to_le_bytes());
        }
        out
    }
}
