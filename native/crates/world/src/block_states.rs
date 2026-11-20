//! What the pinned Minecraft says of each of its block states
//! (`server/block-states.js`), read from the block table
//! (`McbenchBlockTable.java`) and the render layers (`McbenchClient.java`),
//! by state id in the table's order: light emitted and blocked, full faces,
//! face shapes and the pairs that occlude, fluid, drawing bits,
//! skipRendering classes and masks, cull shapes and the pairs that hide, and
//! each block's name. `see_build`'s page reads [`BlockStates::bytes`]; the
//! world route hides blocks with [`BlockStates::of_palette`]; a selection is
//! named with [`BlockStates::names`].

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::Palette;
use crate::js::code_unit_cmp;

/// The block states (see the module's doc).
#[derive(Debug)]
pub struct BlockStates {
    /// The face shapes light is tested through.
    pub shapes: usize,
    /// The face shapes faces are culled by.
    pub cull_shapes: usize,
    /// By pair of face shapes, 1 when they occlude.
    pub occludes: Vec<u8>,
    /// By pair of cull shapes, 1 when the first's face is hidden.
    pub hides: Vec<u8>,
    /// By block id, its default state's properties, in the table's order.
    pub defaults: HashMap<String, Vec<(String, String)>>,
    /// By block id, Minecraft's name of it ("Block of Iron"), in the table's
    /// order.
    pub names: IndexMap<String, String>,
    /// By state name (properties sorted), its state id.
    pub ids: HashMap<String, u16>,
    /// How many states.
    pub count: usize,
    /// By state, the light it emits.
    pub emission: Vec<u8>,
    /// By state, the light it blocks.
    pub light_block: Vec<u8>,
    /// By state, its full faces as bits over the six directions.
    pub full_faces: Vec<u8>,
    /// By state and direction, its face shape.
    pub faces: Vec<u8>,
    /// By state and direction, its cull shape.
    pub cull: Vec<u8>,
    /// By state, its fluid's state (0 for none).
    pub fluid: Vec<u16>,
    /// By state: 1 drawn as a model, 2 its model drawn blended, 4 its fluid
    /// blended, 8 it darkens the corners beside it, 16 drawn through a block
    /// entity renderer.
    pub draw: Vec<u8>,
    /// By state, its skipRendering row class.
    pub skip_row: Vec<u16>,
    /// By state, its skipRendering column class.
    pub skip_column: Vec<u16>,
    /// By row class, the first column class it has a mask for.
    pub skip_first: Vec<u16>,
    /// By row class, how many masks it has.
    pub skip_length: Vec<u16>,
    /// By row class, where its masks start.
    pub skip_start: Vec<u32>,
    /// The masks, bits over down, up, north, south, west, east.
    pub skip_masks: Vec<u8>,
    /// The fluids' states by name, ascending ids (`fluidNames`).
    pub fluid_names: Vec<(u16, String)>,
}

/// By palette entry (entry 0 air), its state id and whether its six faces
/// are full (`ofPalette`'s result).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteStates {
    /// State ids.
    pub ids: Vec<u16>,
    /// 1 where all six faces are full.
    pub occluding: Vec<u8>,
}

fn number<T: std::str::FromStr>(text: &str, what: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("The block table's {what} is {text:?}, not a number"))
}

/// "minecraft:oak_stairs[facing=east,half=bottom]" -> its properties.
fn properties(name: &str) -> Vec<(String, String)> {
    let Some(open) = name.find('[') else {
        return Vec::new();
    };
    let pairs = name[open + 1..name.len() - 1].split(',').map(|pair| {
        let mut parts = pair.split('=');
        (
            parts.next().unwrap_or("").to_string(),
            parts.next().unwrap_or("").to_string(),
        )
    });
    crate::js::object_order(pairs.collect())
}

impl BlockStates {
    /// Reads `table`, the block table's text, and `layers`, the render
    /// layers' text.
    pub fn new(table: &str, layers: &str) -> Result<BlockStates, String> {
        let mut lines: Vec<&str> = table.split('\n').filter(|l| !l.is_empty()).collect();
        let skip_line: Vec<&str> = lines
            .pop()
            .ok_or("The block table is empty")?
            .split('\t')
            .collect();
        let (rows, columns, skip_rows) = (skip_line[1], skip_line[2], &skip_line[3..]);
        let occlude_line: Vec<&str> = lines
            .pop()
            .ok_or("The block table has no face shapes")?
            .split('\t')
            .collect();
        let hide_line: Vec<&str> = lines
            .pop()
            .ok_or("The block table has no cull shapes")?
            .split('\t')
            .collect();
        let shapes: usize = number(occlude_line[1], "face shapes")?;
        let cull_shapes: usize = number(hide_line[1], "cull shapes")?;
        if shapes > 256 || cull_shapes > 256 {
            return Err(format!(
                "The block table has {shapes} face shapes and {cull_shapes} cull shapes, more than a byte numbers"
            ));
        }
        let bits = |text: &str| {
            text.chars()
                .map(|c| u8::from(c == '1'))
                .collect::<Vec<u8>>()
        };
        let occludes = bits(occlude_line[2]);
        let hides = bits(hide_line[2]);
        let mut defaults = HashMap::new();
        let mut names = IndexMap::new();
        let mut states: Vec<Vec<&str>> = Vec::new();
        for line in lines {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields[0] == "default" {
                defaults.insert(fields[1].to_string(), properties(fields[2]));
                names.insert(fields[1].to_string(), fields[3].to_string());
            } else {
                states.push(fields);
            }
        }
        let count = states.len();
        if count > 0x10000
            || number::<usize>(rows, "rows")? > 0x10000
            || number::<usize>(columns, "columns")? > 0x10000
        {
            return Err(
                "The block table has more states or skipRendering classes than 16 bits number"
                    .into(),
            );
        }
        let mut out = BlockStates {
            shapes,
            cull_shapes,
            occludes,
            hides,
            defaults,
            names,
            ids: HashMap::with_capacity(count),
            count,
            emission: vec![0; count],
            light_block: vec![0; count],
            full_faces: vec![0; count],
            faces: vec![0; count * 6],
            cull: vec![0; count * 6],
            fluid: vec![0; count],
            draw: vec![0; count],
            skip_row: vec![0; count],
            skip_column: vec![0; count],
            skip_first: Vec::new(),
            skip_length: Vec::new(),
            skip_start: Vec::new(),
            skip_masks: Vec::new(),
            fluid_names: Vec::new(),
        };
        for (id, fields) in states.iter().enumerate() {
            let [
                name,
                emission,
                light_block,
                full,
                faces,
                fluid,
                model,
                skip,
                cull,
                shades,
            ] = fields[..10]
            else {
                return Err(format!(
                    "The block table's state line {id} has {} fields",
                    fields.len()
                ));
            };
            out.ids.insert(name.to_string(), id as u16);
            out.emission[id] = number(emission, "emission")?;
            out.light_block[id] = number(light_block, "light block")?;
            out.full_faces[id] = number(full, "full faces")?;
            for (direction, shape) in faces.split(',').enumerate() {
                out.faces[id * 6 + direction] = number(shape, "face shape")?;
            }
            for (direction, shape) in cull.split(',').enumerate() {
                out.cull[id * 6 + direction] = number(shape, "cull shape")?;
            }
            out.fluid[id] = number(fluid, "fluid")?;
            out.draw[id] = number::<u8>(model, "model")? | (number::<u8>(shades, "shades")? << 3);
            let (row, column) = skip
                .split_once(',')
                .ok_or_else(|| format!("State {name} has skip {skip:?}"))?;
            out.skip_row[id] = number(row, "skip row")?;
            out.skip_column[id] = number(column, "skip column")?;
        }
        for field in skip_rows {
            let values: Vec<usize> = field
                .split(',')
                .map(|n| number(n, "skip mask"))
                .collect::<Result<_, String>>()?;
            out.skip_first.push(values[0] as u16);
            out.skip_length.push((values.len() - 1) as u16);
            out.skip_start.push(out.skip_masks.len() as u32);
            out.skip_masks.extend(values[1..].iter().map(|&m| m as u8));
        }
        let mut layer_lines: Vec<&str> = layers.split('\n').filter(|l| !l.is_empty()).collect();
        let mut blended = Vec::new();
        while layer_lines
            .first()
            .is_some_and(|l| l.starts_with("layer\t"))
        {
            blended.push(layer_lines.remove(0).split('\t').nth(2) == Some("1"));
        }
        if layer_lines.len() != count {
            return Err(format!(
                "The render layers have {} states, the block table {count}",
                layer_lines.len()
            ));
        }
        for (id, line) in layer_lines.iter().enumerate() {
            let fields: Vec<&str> = line.split('\t').collect();
            let (name, own, fluid, renderer) = (
                fields[0],
                fields.get(1).copied().unwrap_or(""),
                fields.get(2).copied().unwrap_or(""),
                fields.get(3).copied(),
            );
            if states[id][0] != name {
                return Err(format!(
                    "The render layers name state {id} {name}, the block table {}",
                    states[id][0]
                ));
            }
            let layer = |text: &str| {
                text.parse::<usize>()
                    .ok()
                    .and_then(|n| blended.get(n).copied())
                    .unwrap_or(false)
            };
            if layer(own) {
                out.draw[id] |= 2;
            }
            if layer(fluid) {
                out.draw[id] |= 4;
            }
            if renderer != Some("-") {
                out.draw[id] |= 16;
            }
        }
        let mut fluids: Vec<u16> = out.fluid.iter().copied().filter(|&f| f != 0).collect();
        fluids.sort_unstable();
        fluids.dedup();
        out.fluid_names = fluids
            .into_iter()
            .map(|id| (id, states[id as usize][0].to_string()))
            .collect();
        Ok(out)
    }

    /// Block.shouldRenderFace: whether state `a` hides its face toward state
    /// `b`, its neighbour to `direction` (`hidesFace`).
    pub fn hides_face(&self, a: usize, b: usize, direction: usize) -> bool {
        self.hides[self.cull[a * 6 + direction] as usize * self.cull_shapes
            + self.cull[b * 6 + (direction ^ 1)] as usize]
            == 1
            || self.skips(a, b, direction)
    }

    /// BlockState.skipRendering (`skips`).
    pub fn skips(&self, a: usize, b: usize, direction: usize) -> bool {
        let row = self.skip_row[a] as usize;
        let at = self.skip_column[b] as i64 - self.skip_first[row] as i64;
        at >= 0
            && at < self.skip_length[row] as i64
            && (self.skip_masks[self.skip_start[row] as usize + at as usize] >> direction) & 1 == 1
    }

    /// The state id of block `id` with `properties`, its block's defaults
    /// where it names none (`idOf`), with today's errors.
    pub fn id_of(&self, id: &str, properties: &[(String, String)]) -> Result<u16, String> {
        let defaults = self
            .defaults
            .get(id)
            .ok_or_else(|| format!("Minecraft has no block {id}"))?;
        let mut merged = defaults.clone();
        merged.extend(properties.iter().cloned());
        let mut merged = crate::js::object_order(merged);
        merged.sort_by(|a, b| code_unit_cmp(&a.0, &b.0));
        let name = if merged.is_empty() {
            id.to_string()
        } else {
            format!(
                "{id}[{}]",
                merged
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        self.ids
            .get(&name)
            .copied()
            .ok_or_else(|| format!("Minecraft has no block state {name}"))
    }

    /// For each entry of a world's palette, its state id and whether its six
    /// faces are full (`ofPalette`).
    pub fn of_palette(&self, palette: &Palette) -> Result<PaletteStates, String> {
        let size = palette.len();
        let mut ids = vec![0u16; size];
        let mut occluding = vec![0u8; size];
        for index in 1..size {
            let entry = palette.entry(index);
            ids[index] = self.id_of(&entry.id, &entry.properties)?;
            occluding[index] = u8::from(self.full_faces[ids[index] as usize] == 63);
        }
        Ok(PaletteStates { ids, occluding })
    }

    /// What the page reads (`bytes`), each part starting on four bytes: u32
    /// states, face shapes, cull shapes, row classes, mask bytes and fluid
    /// names' bytes; by state u16 fluid, row and column; by row class u32
    /// start, u16 first and length; by state emission, light block, full
    /// faces, draw bits, six face shapes, six cull shapes; the shape pairs,
    /// the cull pairs, the masks and the fluids' names (JSON).
    pub fn bytes(&self) -> Vec<u8> {
        let mut names = String::from("{");
        for (at, (id, name)) in self.fluid_names.iter().enumerate() {
            if at > 0 {
                names.push(',');
            }
            names.push_str(&format!("\"{id}\":"));
            crate::js::push_json_string(&mut names, name);
        }
        names.push('}');
        let mut out = Vec::new();
        let part = |out: &mut Vec<u8>, bytes: Vec<u8>| {
            out.extend_from_slice(&bytes);
            out.resize(out.len() + (4 - bytes.len() % 4) % 4, 0);
        };
        let u16s = |v: &[u16]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let u32s = |v: &[u32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        part(
            &mut out,
            u32s(&[
                self.count as u32,
                self.shapes as u32,
                self.cull_shapes as u32,
                self.skip_first.len() as u32,
                self.skip_masks.len() as u32,
                names.len() as u32,
            ]),
        );
        part(&mut out, u16s(&self.fluid));
        part(&mut out, u16s(&self.skip_row));
        part(&mut out, u16s(&self.skip_column));
        part(&mut out, u32s(&self.skip_start));
        part(&mut out, u16s(&self.skip_first));
        part(&mut out, u16s(&self.skip_length));
        for bytes in [
            &self.emission,
            &self.light_block,
            &self.full_faces,
            &self.draw,
            &self.faces,
            &self.cull,
            &self.occludes,
            &self.hides,
            &self.skip_masks,
        ] {
            part(&mut out, bytes.clone());
        }
        part(&mut out, names.into_bytes());
        out
    }
}
