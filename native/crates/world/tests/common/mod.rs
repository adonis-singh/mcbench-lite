//! What the tests that need Node, a data folder or Minecraft's tables share:
//! the environment variables that turn them on, which fail the test when
//! set and something is missing.

#![allow(dead_code)]

pub mod block_table;
pub mod fake_facts;
#[path = "../../../agent/tests/common/fixture.rs"]
pub mod fixture;

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// sha256 of `bytes`, hex.
pub fn sha(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes.as_ref())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The engine's table, `blocks.txt` in the folder `MCBENCH_TABLES` names
/// (a copy of `core/minecraft/1.21.10/`), or `None` when it is not set.
pub fn tables() -> Option<PathBuf> {
    let folder = folder("MCBENCH_TABLES")?;
    let file = folder.join("blocks.txt");
    assert!(
        file.is_file(),
        "MCBENCH_TABLES names {}, which has no blocks.txt",
        folder.display()
    );
    Some(file)
}

/// [`tables`], which a test that is on needs.
pub fn blocks_txt() -> PathBuf {
    tables()
        .expect("set MCBENCH_TABLES to the folder of Minecraft's tables (core/minecraft/1.21.10)")
}

/// The folder named by environment variable `name`, which must exist.
pub fn folder(name: &str) -> Option<PathBuf> {
    let value = std::env::var_os(name)?;
    let folder = PathBuf::from(value);
    assert!(
        folder.is_dir(),
        "{name} names {}, which is not a folder",
        folder.display()
    );
    Some(folder)
}

/// Runs a Node script of `tests/node/` with `args`, from the checkout
/// `root` (whose `node_modules` it uses), and fails on a non-zero exit.
pub fn node(root: &Path, script: &str, args: &[&Path]) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/node")
        .join(script);
    let status = Command::new("node")
        .arg("--max-old-space-size=12000")
        .arg(&script)
        .arg(root)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("cannot run node: {e}"));
    assert!(status.success(), "{} failed: {status}", script.display());
}

/// The JSON at `file`.
pub fn json(file: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(file)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", file.display()))
}

/// Node's `attempt`: `{ value }`, `{}` for no value, or `{ error }`.
pub fn attempt(result: Result<serde_json::Value, String>) -> serde_json::Value {
    match result {
        Ok(serde_json::Value::Null) => serde_json::json!({}),
        Ok(value) => serde_json::json!({ "value": value }),
        Err(error) => serde_json::json!({ "error": error }),
    }
}

/// A list of differences, by what was compared.
#[derive(Default)]
pub struct Report {
    /// How many comparisons by kind, and how many were identical.
    pub counts: std::collections::BTreeMap<String, (usize, usize)>,
    /// The differences.
    pub differences: Vec<String>,
}

impl Report {
    /// Compares `node` and `rust` as JSON text, under `kind` for `what`.
    pub fn check(
        &mut self,
        kind: &str,
        what: &str,
        node: &serde_json::Value,
        rust: &serde_json::Value,
    ) {
        let (a, b) = (
            serde_json::to_string(node).expect("JSON"),
            serde_json::to_string(rust).expect("JSON"),
        );
        let count = self.counts.entry(kind.to_string()).or_default();
        count.0 += 1;
        if a == b {
            count.1 += 1;
        } else {
            let cut = |s: &str| {
                if s.len() > 600 {
                    format!(
                        "{}...",
                        &s[..s.char_indices().nth(600).map_or(s.len(), |(i, _)| i)]
                    )
                } else {
                    s.to_string()
                }
            };
            self.differences.push(format!(
                "{kind} {what}:\n  node {}\n  rust {}",
                cut(&a),
                cut(&b)
            ));
        }
    }

    /// Prints the table and fails if anything differed.
    pub fn finish(self, title: &str) {
        println!("{title}:");
        for (kind, (checked, same)) in &self.counts {
            println!("  {kind}: {checked} checked, {same} identical");
        }
        for difference in self.differences.iter().take(40) {
            println!("{difference}");
        }
        assert!(
            self.differences.is_empty(),
            "{} differences",
            self.differences.len()
        );
    }
}
