//! Compiles the updater's feed into the binary: `MCBENCH_UPDATE_KEY_FILE`
//! names a minisign public key file (`minisign.pub`'s two lines) and
//! `MCBENCH_UPDATE_FEED` the address of `latest.json`. Neither set, the
//! crate builds with no updater; one without the other fails the build.

use std::{env, fs, path::Path};

const KEY: &str = "MCBENCH_UPDATE_KEY_FILE";
const FEED: &str = "MCBENCH_UPDATE_FEED";

fn var(name: &str) -> Result<Option<String>, String> {
    println!("cargo:rerun-if-env-changed={name}");
    match env::var(name) {
        Err(env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(format!("{name} is not valid text: {e}")),
        Ok(value) => Ok(Some(value)),
    }
}

fn feed() -> Result<Option<(String, String)>, String> {
    let (path, url) = match (var(KEY)?, var(FEED)?) {
        (None, None) => return Ok(None),
        (Some(path), Some(url)) => (path, url),
        (Some(_), None) => {
            return Err(format!(
                "{KEY} is set and {FEED} is not: an updater build names both its key and the address of its latest.json."
            ));
        }
        (None, Some(_)) => {
            return Err(format!(
                "{FEED} is set and {KEY} is not: an updater build names both its key and the address of its latest.json."
            ));
        }
    };
    println!("cargo:rerun-if-changed={path}");
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("{KEY} names {path}, which cannot be read: {e}"))?;
    minisign_verify::PublicKey::decode(&text)
        .map_err(|e| format!("{KEY} names {path}, which is not a minisign public key: {e}"))?;
    let line = text.lines().nth(1).expect("decode read a second line");
    Ok(Some((line.trim().to_string(), url)))
}

fn main() {
    let feed = match feed() {
        Ok(feed) => feed,
        Err(sentence) => {
            println!("cargo::error={sentence}");
            return;
        }
    };
    let out = Path::new(&env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("update_feed.rs");
    fs::write(
        out,
        format!("/// The compiled-in update key and feed address.\nconst UPDATE_FEED: Option<(&str, &str)> = {feed:?};\n"),
    )
    .expect("OUT_DIR is writable");
}
