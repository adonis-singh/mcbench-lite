//! `export::schematic` against today's `writeSchematic` on every stored
//! world: reads `tests/node/schematic.js`'s lines and compares the sha256 of
//! the bytes (gzip) and of the NBT inside, or the error.
//!
//! ```text
//! schematic <data clone> <node.jsonl>
//! ```

use std::io::Read;
use std::path::PathBuf;

use serde_json::Value;
use sha2::{Digest, Sha256};

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, data, node] = &args[..] else {
        panic!("usage: schematic <data clone> <node.jsonl>")
    };
    let data = PathBuf::from(data);
    let (mut worlds, mut same_gzip, mut same_nbt, mut same_error) = (0, 0, 0, 0);
    let mut differing = Vec::new();
    for line in std::fs::read_to_string(node).unwrap().lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        worlds += 1;
        let chunks = data
            .join("worlds")
            .join(row["owner"].as_str().unwrap())
            .join("chunks");
        let world =
            mcbench_tools::testing::world_of(row["manifest"].as_str().unwrap(), &chunks).unwrap();
        match mcbench_tools::export::schematic(&world) {
            Ok(bytes) => {
                let mut nbt = Vec::new();
                flate2::read::GzDecoder::new(&bytes[..])
                    .read_to_end(&mut nbt)
                    .unwrap();
                if row["gzip"].as_str() == Some(&sha(&bytes)) {
                    same_gzip += 1;
                }
                if row["nbt"].as_str() == Some(&sha(&nbt)) {
                    same_nbt += 1;
                } else {
                    differing.push(format!(
                        "{}: nbt differs (node {} bytes, rust {})",
                        row["key"],
                        row["bytes"],
                        bytes.len()
                    ));
                }
            }
            Err(error) => {
                if row["error"].as_str() == Some(&error) {
                    same_error += 1;
                } else {
                    differing.push(format!(
                        "{}: rust error {error:?}, node {}",
                        row["key"], row["error"]
                    ));
                }
            }
        }
    }
    println!(
        "{worlds} worlds: {same_gzip} gzip byte-identical, {same_nbt} NBT identical, {same_error} errors identical"
    );
    for line in &differing {
        println!("  {line}");
    }
}
