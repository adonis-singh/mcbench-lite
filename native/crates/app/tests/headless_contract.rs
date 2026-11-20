//! `mcbench-lite headless contract`, the built binary, under both of the
//! sweep's budget regimes, gives the sweep sha256 the benchmark pins.

use std::process::Command;

use mcbench_agent::contract::{self, Budgets, Regime};

fn run(regime: Regime) -> serde_json::Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcbench-lite"));
    command.args(["headless", "contract"]);
    for name in Budgets::ENV {
        command.env_remove(name);
    }
    command.envs(regime.vars());
    let output = command.output().expect("mcbench-lite runs");
    assert!(
        output.status.success(),
        "headless contract: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("UTF-8");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "one JSON object on one line");
    mcbench_json::parse(lines[0]).expect("JSON")
}

#[test]
fn both_regimes_give_the_pinned_sweep_sha256() {
    let mut attempts = Vec::new();
    for regime in Regime::ALL {
        let output = run(regime);
        let budgets = regime.budgets();
        assert_eq!(
            mcbench_json::stringify(&output["budgets"]),
            mcbench_json::to_string(&budgets).unwrap()
        );
        for (name, form) in contract::benchmark_forms() {
            let part = &output["forms"][name];
            let ours = contract::of(form, &budgets).unwrap();
            // What a client reads back and writes again is what was hashed.
            assert_eq!(mcbench_json::stringify(&part["tools"]), ours.tools_json());
            assert_eq!(
                part["systemPrompt"].as_str(),
                Some(ours.system_prompt.as_str())
            );
            attempts.push((
                format!("{}/{name}", regime.name()),
                part["attempt"]["sha256"].as_str().unwrap().to_owned(),
            ));
        }
    }
    assert_eq!(attempts.len(), 8);
    assert_eq!(
        contract::sweep_of(&attempts),
        contract::TOOL_CONTRACT_SHA256
    );
}

#[test]
fn a_refused_budget_exits_1_with_read_budgets_words() {
    let output = Command::new(env!("CARGO_BIN_EXE_mcbench-lite"))
        .args(["headless", "contract"])
        .env("MCBENCH_VERIFY_MAX_TESTS", "0")
        .output()
        .expect("mcbench-lite runs");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "mcbench-lite headless contract: MCBENCH_VERIFY_MAX_TESTS must be an integer within 1..2147483647\n"
    );
}
