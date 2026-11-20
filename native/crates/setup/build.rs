//! With the feature `shipped-data` (on in every release build): the bundle
//! that `MCBENCH_DATA_BUNDLE` names, checked against
//! `native/minecraft-data.lock` (its sha256 and size), copied to the build's
//! output folder for `include_bytes!`. Without the feature, nothing.

use std::path::PathBuf;

use sha2::Digest;

fn main() {
    let lock =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
            .join("../../minecraft-data.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    println!("cargo:rerun-if-env-changed=MCBENCH_DATA_BUNDLE");
    if std::env::var_os("CARGO_FEATURE_SHIPPED_DATA").is_none() {
        return;
    }
    let text = std::fs::read_to_string(&lock).unwrap_or_else(|e| panic!("{}: {e}", lock.display()));
    let (sha256, size) = text
        .lines()
        .find_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            match fields[..] {
                ["bundle", sha256, size] => Some((
                    sha256.to_string(),
                    size.parse::<u64>()
                        .expect("the lock file's bundle size is a number"),
                )),
                _ => None,
            }
        })
        .expect("the lock file names the bundle");
    let url = text
        .lines()
        .find_map(|line| line.strip_prefix("url\t"))
        .unwrap_or("(no address)");
    let Some(bundle) = std::env::var_os("MCBENCH_DATA_BUNDLE") else {
        panic!(
            "The feature shipped-data embeds the Minecraft data bundle, and MCBENCH_DATA_BUNDLE names none. Make it with native/tools/extract/bundle.sh (native/tools/extract/README.md) or download the release asset native/minecraft-data.lock names (its url line: {url}), then set MCBENCH_DATA_BUNDLE to its path; its sha256 must be {sha256}."
        )
    };
    let bundle = PathBuf::from(bundle);
    println!("cargo:rerun-if-changed={}", bundle.display());
    let bytes = std::fs::read(&bundle)
        .unwrap_or_else(|e| panic!("MCBENCH_DATA_BUNDLE {}: {e}", bundle.display()));
    let got: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != sha256 || bytes.len() as u64 != size {
        panic!(
            "MCBENCH_DATA_BUNDLE {} has sha256 {got} and {} bytes, but native/minecraft-data.lock names {sha256} and {size} bytes.",
            bundle.display(),
            bytes.len()
        );
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"))
        .join("minecraft-data.tar.zst");
    std::fs::write(&out, &bytes).unwrap_or_else(|e| panic!("{}: {e}", out.display()));
}
