//! Release tooling for the shipped Minecraft data
//! (`native/tools/extract/README.md`), and the measurements of the first
//! start. Nothing here runs in the app.
//!
//! ```text
//! mcbench-data stage <cache> <work> <folder>      the shipped files, by their bundle names, client-jar.json made
//! mcbench-data tar <folder> <archive>            the bundle's archive of a staged folder
//! mcbench-data lock <bundle> <folder> <zstd version> <url>   the lock file's text
//! mcbench-data check <bundle> <lock> [<folder>]  the bundle against a lock file (and a staged folder)
//! mcbench-data bake <folder> <client jar> <out>  the bake from a whole local jar
//! mcbench-data compare <out> <today's bake> <mcmeta folder>  the bake against today's
//! mcbench-data first-start <bundle> <lock> <data folder> [--rate <bytes per second>]  a timed first start
//! ```

mod compare;
mod release;

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = |i: usize| PathBuf::from(args.get(i).cloned().unwrap_or_default());
    let result = match args.first().map(String::as_str) {
        Some("stage") if args.len() == 4 => release::stage(&path(1), &path(2), &path(3)),
        Some("tar") if args.len() == 3 => release::tar(&path(1), &path(2)),
        Some("lock") if args.len() == 5 => release::lock(&path(1), &path(2), &args[3], &args[4]),
        Some("check") if args.len() == 3 || args.len() == 4 => release::check(&path(1), &path(2), args.get(3).map(PathBuf::from)),
        Some("bake") if args.len() == 4 => compare::bake_local(&path(1), &path(2), &path(3)),
        Some("compare") if args.len() == 4 => compare::compare(&path(1), &path(2), &path(3)),
        Some("first-start") if args.len() == 4 || args.len() == 6 => {
            let rate = (args.len() == 6 && args[4] == "--rate").then(|| args[5].parse::<f64>().expect("--rate takes bytes per second"));
            compare::first_start(&path(1), &path(2), &path(3), rate)
        }
        _ => Err("usage: mcbench-data stage|tar|lock|check|bake|compare|first-start ... (see its source's header)".to_string()),
    };
    if let Err(error) = result {
        eprintln!("mcbench-data: {error}");
        std::process::exit(1);
    }
}
