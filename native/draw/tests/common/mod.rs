// What tests share: the data folder with Minecraft's baked data, when the
// machine has one (MCBENCH_DRAW_DATA names it: a data folder of today's
// bake), and a GPU adapter.
#![allow(dead_code)]

use std::path::PathBuf;

// The data folder, or None with the reason printed: the test then passes
// without checking anything, as the repository's tests skip without
// Minecraft's data.
pub fn data(test: &str) -> Option<PathBuf> {
    match std::env::var_os("MCBENCH_DRAW_DATA") {
        None => {
            eprintln!("{test}: skipped: MCBENCH_DRAW_DATA does not name a data folder with Minecraft's baked data");
            None
        }
        Some(folder) => {
            let folder = PathBuf::from(folder);
            assert!(
                folder.join("minecraft-live/bluemap").is_dir(),
                "MCBENCH_DRAW_DATA={} holds no bake (minecraft-live/bluemap)",
                folder.display()
            );
            Some(folder)
        }
    }
}

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// The adapter tests draw with: the platform's hardware one, or with
// MCBENCH_DRAW_SOFTWARE=1 the software floor (on Linux the SwiftShader
// library MCBENCH_DRAW_SWIFTSHADER names: what the server passes as
// softwareVulkan; Linux x86_64 runs give it).
pub fn choice() -> mcbench_draw::gpu::Choice {
    mcbench_draw::gpu::Choice {
        software: software(),
        software_vulkan: software_vulkan(),
    }
}

pub fn software() -> bool {
    std::env::var("MCBENCH_DRAW_SOFTWARE").is_ok_and(|v| v == "1")
}

pub fn software_vulkan() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let path = PathBuf::from(std::env::var_os("MCBENCH_DRAW_SWIFTSHADER")?);
    assert!(
        path.is_file(),
        "MCBENCH_DRAW_SWIFTSHADER={} is not a file",
        path.display()
    );
    Some(path)
}

// A blueprint of named block states ("id[name=value,...]") at Minecraft
// positions: the palette is air and the names in the order first used.
pub fn blueprint(blocks: &[(&str, [i32; 3])]) -> Vec<u8> {
    let mut names: Vec<&str> = vec!["minecraft:air"];
    for (n, _) in blocks {
        if !names.contains(n) {
            names.push(n);
        }
    }
    let utf = |out: &mut Vec<u8>, s: &str| {
        out.extend_from_slice(&(s.len() as u16).to_be_bytes());
        out.extend_from_slice(s.as_bytes());
    };
    let mut out = Vec::new();
    out.extend_from_slice(&(names.len() as i32).to_be_bytes());
    for n in &names {
        let (id, props) = match n.split_once('[') {
            Some((id, rest)) => (
                id,
                rest.trim_end_matches(']')
                    .split(',')
                    .filter(|p| !p.is_empty())
                    .map(|p| p.split_once('=').unwrap())
                    .collect::<Vec<_>>(),
            ),
            None => (*n, Vec::new()),
        };
        utf(&mut out, id);
        out.extend_from_slice(&(props.len() as i32).to_be_bytes());
        for (k, v) in props {
            utf(&mut out, k);
            utf(&mut out, v);
        }
    }
    let mut sections: std::collections::BTreeMap<[i32; 3], Vec<u8>> = Default::default();
    for (n, b) in blocks {
        let v = names.iter().position(|x| x == n).unwrap() as u16;
        let cells = sections
            .entry(b.map(|c| c >> 4))
            .or_insert_with(|| vec![0; 8192]);
        let i = (((b[1] & 15) << 8) | ((b[2] & 15) << 4) | (b[0] & 15)) as usize;
        cells[i * 2..i * 2 + 2].copy_from_slice(&v.to_be_bytes());
    }
    let lo = [0, 1, 2].map(|k| blocks.iter().map(|(_, b)| b[k]).min().unwrap());
    let hi = [0, 1, 2].map(|k| blocks.iter().map(|(_, b)| b[k]).max().unwrap());
    for v in lo.iter().chain(hi.iter()) {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&(sections.len() as i32).to_be_bytes());
    for (s, cells) in sections {
        for v in s {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&cells);
    }
    out
}
