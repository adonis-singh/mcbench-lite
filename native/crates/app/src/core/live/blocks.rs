//! The blocks of the world on show, as the engine wrote them: the
//! blueprint of its last placement and every patch since (`web/live-patch.mjs`
//! `readBlueprint`, `readPatch` and `InputBlocks`), with the input each
//! palette entry takes. A click is named and sent from here: a block whose
//! state takes no input is not sent to the engine, as the page sends none.

use std::collections::HashMap;

/// A section's blocks, palette indices in the blueprint's order
/// (`((y & 15) << 8) | ((z & 15) << 4) | (x & 15)`).
type Section = Box<[u16; 4096]>;

/// The world on show (see the module's doc).
#[derive(Default)]
pub struct LiveBlocks {
    ids: Vec<String>,
    inputs: Vec<Option<&'static str>>,
    sections: HashMap<[i32; 3], Section>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at + n;
        let taken = self
            .bytes
            .get(self.at..end)
            .ok_or("The engine's bytes end too early")?;
        self.at = end;
        Ok(taken)
    }
    fn int(&mut self) -> Result<i32, String> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn short(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn utf(&mut self) -> Result<String, String> {
        let n = self.short()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|e| e.to_string())
    }
    /// Palette entries as blueprints write them: their ids.
    fn entries(&mut self) -> Result<Vec<String>, String> {
        let mut ids = Vec::new();
        for _ in 0..self.int()? {
            ids.push(self.utf()?);
            for _ in 0..self.int()? {
                self.utf()?;
                self.utf()?;
            }
        }
        Ok(ids)
    }
}

/// The engine's input bytes (`Stream::inputs`, `CoreWorld.inputs`'s
/// codes): 0 none, 1 toggle, 2 press, 3 use.
fn inputs_of(bytes: &[u8]) -> Result<Vec<Option<&'static str>>, String> {
    bytes
        .iter()
        .map(|code| match code {
            0 => Ok(None),
            1 => Ok(Some("toggle")),
            2 => Ok(Some("press")),
            3 => Ok(Some("use")),
            other => Err(format!(
                "The engine named input {other}, which is none of 0 to 3"
            )),
        })
        .collect()
}

fn at(pos: [i32; 3]) -> ([i32; 3], usize) {
    (
        [pos[0] >> 4, pos[1] >> 4, pos[2] >> 4],
        (((pos[1] & 15) << 8) | ((pos[2] & 15) << 4) | (pos[0] & 15)) as usize,
    )
}

impl LiveBlocks {
    /// A placement's blueprint and the input of each entry of its palette
    /// (`InputBlocks.load`).
    pub fn load(blueprint: &[u8], inputs: &[u8]) -> Result<LiveBlocks, String> {
        let mut reader = Reader {
            bytes: blueprint,
            at: 0,
        };
        let ids = reader.entries()?;
        let inputs = inputs_of(inputs)?;
        if inputs.len() != ids.len() {
            return Err(format!(
                "The engine named the inputs of {} palette entries, and its blueprint has {}",
                inputs.len(),
                ids.len()
            ));
        }
        reader.take(24)?;
        let mut sections = HashMap::new();
        for _ in 0..reader.int()? {
            let key = [reader.int()?, reader.int()?, reader.int()?];
            let cells = reader.take(8192)?;
            let mut section: Section = Box::new([0; 4096]);
            for (i, cell) in section.iter_mut().enumerate() {
                *cell = u16::from_be_bytes([cells[i * 2], cells[i * 2 + 1]]);
            }
            sections.insert(key, section);
        }
        Ok(LiveBlocks {
            ids,
            inputs,
            sections,
        })
    }

    /// A tick's or an edit's patch and the input of each palette entry it
    /// adds (`InputBlocks.apply`).
    pub fn apply(&mut self, patch: &[u8], inputs: &[u8]) -> Result<(), String> {
        let mut reader = Reader {
            bytes: patch,
            at: 0,
        };
        let added = reader.entries()?;
        let inputs = inputs_of(inputs)?;
        if inputs.len() != added.len() {
            return Err(format!(
                "The engine named the inputs of {} palette entries, and its patch adds {}",
                inputs.len(),
                added.len()
            ));
        }
        self.ids.extend(added);
        self.inputs.extend(inputs);
        reader.take(28)?;
        for _ in 0..reader.int()? {
            let pos = [reader.int()?, reader.int()?, reader.int()?];
            let index = reader.short()?;
            if index as usize >= self.ids.len() {
                return Err(format!(
                    "The patch sets palette entry {index}, past its {} entries",
                    self.ids.len()
                ));
            }
            let (key, cell) = at(pos);
            self.sections
                .entry(key)
                .or_insert_with(|| Box::new([0; 4096]))[cell] = index;
        }
        if reader.at != patch.len() {
            return Err("The patch has trailing bytes".into());
        }
        Ok(())
    }

    /// The block at `pos`: its id and the input its state takes.
    pub fn at(&self, pos: [i32; 3]) -> (&str, Option<&'static str>) {
        let (key, cell) = at(pos);
        let index = self.sections.get(&key).map_or(0, |s| s[cell]) as usize;
        (&self.ids[index], self.inputs[index])
    }
}
