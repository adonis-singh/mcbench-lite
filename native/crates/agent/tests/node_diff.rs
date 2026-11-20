//! The contract against Node's, byte for byte. Needs Node (`node` on the
//! PATH) and a checkout whose `server/` loads (it has `node_modules`):
//!
//! `MCBENCH_NODE_DIFF=<checkout> MCBENCH_BENCH_DIR=<folder> cargo test -p mcbench-agent --test node_diff -- --ignored`
//!
//! `MCBENCH_BENCH_DIR` names the private benchmark's folder, which the
//! generator reads the import order revision and the unrestricted study's
//! budgets from (`generate.js --bench`); a checkout without the benchmark
//! cannot run this test, and with `MCBENCH_NODE_DIFF` set and no
//! `MCBENCH_BENCH_DIR` it fails saying so.
//!
//! It runs `contract/generate.js --check` against that checkout's `server/`
//! (the committed files are what the generator writes), then, under the
//! default budgets, the unrestricted study's and 40 random budget
//! environments (some refused), compares for every placement, image input,
//! network access, chat and resolution the tools, the system prompt and the
//! budgets with what `generate.js --dump` prints, and the refusals' words.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use mcbench_agent::contract::{self, Budgets, Form, Placement, Regime};
use serde_json::Value;

fn generator() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("contract/generate.js")
}

fn checkout() -> String {
    let root = std::env::var("MCBENCH_NODE_DIFF")
        .expect("MCBENCH_NODE_DIFF names a checkout whose server/ loads (with node_modules)");
    assert!(
        PathBuf::from(&root).join("node_modules").is_dir(),
        "{root} has no node_modules"
    );
    root
}

fn bench() -> String {
    let folder = std::env::var("MCBENCH_BENCH_DIR")
        .expect("MCBENCH_BENCH_DIR names the private benchmark's folder (generate.js --bench)");
    assert!(
        PathBuf::from(&folder).is_dir(),
        "MCBENCH_BENCH_DIR {folder} is not a folder"
    );
    folder
}

/// `generate.js --dump` under exactly these budget variables.
fn node_dump(root: &str, vars: &[(String, String)]) -> Value {
    let mut command = Command::new("node");
    command
        .arg(generator())
        .args(["--dump", "--root", root, "--bench", &bench()]);
    for name in Budgets::ENV {
        command.env_remove(name);
    }
    command.envs(vars.iter().map(|(k, v)| (k, v)));
    let output = command.output().expect("node runs");
    assert!(
        output.status.success(),
        "generate.js --dump failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("--dump prints JSON")
}

/// A small fixed-seed generator for the random environments.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A random budget environment: each variable set or not, to a valid value
/// spread over every magnitude (written as JavaScript's `Number` reads it in
/// several ways), and in some environments one invalid value.
fn random_vars(rng: &mut Rng, refused: bool) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    for name in Budgets::ENV {
        if rng.below(10) < 3 {
            continue;
        }
        let maximum: u64 = if name == "MCBENCH_VERIFY_MAX_REPORT_BYTES" {
            (1 << 53) - 1
        } else {
            2_147_483_647
        };
        let value = match rng.below(6) {
            0 => maximum,
            1 => 1 + rng.below(1000),
            _ => {
                let bits = 1 + rng.below(u64::from(64 - maximum.leading_zeros()));
                1 + rng.below(1 << bits)
            }
        }
        .min(maximum);
        let text = match rng.below(8) {
            0 => format!(" {value}\n"),
            1 => format!("0x{value:x}"),
            2 => format!("+{value}.0"),
            3 if value % 1000 == 0 => format!("{}e3", value / 1000),
            _ => value.to_string(),
        };
        vars.push((name.to_owned(), text));
    }
    if refused {
        let bad = [
            "0",
            "-1",
            "1.5",
            "2147483648",
            "abc",
            "",
            "Infinity",
            "0x",
            "1e400",
        ];
        let name = Budgets::ENV[rng.below(7) as usize];
        vars.retain(|(n, _)| n != name);
        vars.push((
            name.to_owned(),
            bad[rng.below(bad.len() as u64) as usize].to_owned(),
        ));
    }
    vars
}

fn text(value: &Value) -> &str {
    value.as_str().expect("a string")
}

#[test]
#[ignore = "needs Node, MCBENCH_NODE_DIFF=<checkout with node_modules> and MCBENCH_BENCH_DIR=<the benchmark's folder>"]
fn generated_files_and_every_form_are_nodes() {
    let root = checkout();
    let check = Command::new("node")
        .arg(generator())
        .args(["--check", "--root", &root, "--bench", &bench()])
        .output()
        .expect("node runs");
    assert!(
        check.status.success(),
        "generate.js --check: {}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );

    let mut rng = Rng(0x6d63_6265_6e63_6801);
    let mut environments: Vec<Vec<(String, String)>> =
        Regime::ALL.iter().map(|r| r.vars()).collect();
    for n in 0..40 {
        environments.push(random_vars(&mut rng, n % 8 == 7));
    }
    // Quotients that print with long fractions.
    environments.push(vec![
        ("MCBENCH_VERIFY_MAX_WALL_MS".into(), "2147483647".into()),
        (
            "MCBENCH_VERIFY_MAX_REQUEST_BYTES".into(),
            "2147483647".into(),
        ),
        (
            "MCBENCH_VERIFY_MAX_REPORT_BYTES".into(),
            "9007199254740991".into(),
        ),
    ]);

    let (mut compared, mut refused) = (0, 0);
    for vars in &environments {
        let node = node_dump(&root, vars);
        let map: HashMap<&str, OsString> =
            vars.iter().map(|(k, v)| (k.as_str(), v.into())).collect();
        let ours = Budgets::from_vars(|name| map.get(name).cloned());
        if node["ok"] == Value::Bool(false) {
            assert_eq!(
                ours,
                Err(text(&node["error"]).to_owned()),
                "refusal under {vars:?}"
            );
            refused += 1;
            continue;
        }
        let budgets = ours.unwrap_or_else(|e| panic!("we refused {vars:?} ({e}); Node did not"));
        assert_eq!(node["importOrder"].as_u64(), Some(contract::import_order()));
        let resolutions: Vec<u32> = node["resolutions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_u64().unwrap() as u32)
            .collect();
        let standard = contract::benchmark_forms()[0].1.resolution;
        let three = Form {
            resolution: 3,
            ..contract::benchmark_forms()[0].1
        };
        assert_eq!(
            contract::of(three, &budgets).unwrap_err(),
            text(&node["resolutionError"])
        );
        for placement in [Placement::Updates, Placement::Strict] {
            for image_input in [true, false] {
                for network_access in [true, false] {
                    for chat in [false, true] {
                        for &resolution in &resolutions {
                            let form = Form {
                                placement,
                                image_input,
                                network_access,
                                chat,
                                resolution,
                            };
                            let contract = contract::of(form, &budgets).unwrap();
                            assert_eq!(
                                contract.budgets,
                                text(&node["budgets"]),
                                "budgets under {vars:?}"
                            );
                            let image = if image_input { "image" } else { "text" };
                            let network = if network_access { "network" } else { "offline" };
                            let tools = format!(
                                "{}/{image}/{network}{}",
                                placement.name(),
                                if chat { "/chat" } else { "" }
                            );
                            assert_eq!(
                                contract.tools_json(),
                                text(&node["tools"][&tools]),
                                "tools {tools} under {vars:?}"
                            );
                            let prompt = format!("{image}/{network}/{resolution}");
                            assert_eq!(
                                contract.system_prompt,
                                text(&node["prompts"][&prompt]),
                                "prompt {prompt} under {vars:?}"
                            );
                            compared += 1;
                        }
                    }
                }
            }
        }
        assert!(resolutions.contains(&standard));
    }
    eprintln!(
        "{} environments: {} compared forms identical, {} refusals identical",
        environments.len(),
        compared,
        refused
    );
    assert!(environments.len() - refused >= 30);
}
