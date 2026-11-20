//! `mcbench-lite headless contract`: the tool contract of this process's
//! budget regime, for the benchmark's thin client (`PROTOCOL.md`, family
//! `contract`). It replaces `tool-contract.js` `regimeAttempts`' requires of
//! `server/agent`, `server/prompt` and `server/verification-contract`.

use mcbench_agent::contract::{self, Budgets};

/// Prints the contract of this process's environment and exits 0; exits 1
/// with `readBudgets`' error when a budget is refused, 2 when given words.
pub(crate) fn run(arguments: &[String]) -> ! {
    if !arguments.is_empty() {
        eprintln!(
            "mcbench-lite headless contract: takes no arguments, not {}",
            arguments.join(" ")
        );
        std::process::exit(2)
    }
    let budgets = match Budgets::from_env() {
        Ok(budgets) => budgets,
        Err(message) => {
            eprintln!("mcbench-lite headless contract: {message}");
            std::process::exit(1)
        }
    };
    println!("{}", output(&budgets));
    std::process::exit(0)
}

/// The one JSON object: `{"budgets": BUDGETS, "forms": {"<form>": {"tools",
/// "systemPrompt", "attempt"}}}`, the tools and budgets as the bytes the
/// hash covers.
fn output(budgets: &Budgets) -> String {
    let quote = |text: &str| mcbench_json::to_string(text).expect("a string serializes");
    let mut forms = Vec::new();
    let mut budgets_json = None;
    for (name, form) in contract::benchmark_forms() {
        let contract = contract::of(form, budgets).expect("the benchmark's forms are valid");
        let attempt =
            mcbench_json::to_string(&contract::attempt(&contract)).expect("an attempt serializes");
        forms.push(format!(
            "{}:{{\"tools\":{},\"systemPrompt\":{},\"attempt\":{attempt}}}",
            quote(name),
            contract.tools_json(),
            quote(&contract.system_prompt),
        ));
        budgets_json = Some(contract.budgets);
    }
    format!(
        "{{\"budgets\":{},\"forms\":{{{}}}}}",
        budgets_json.expect("four forms"),
        forms.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_is_one_object_whose_parts_round_trip() {
        let budgets = contract::Regime::Standard.budgets();
        let text = output(&budgets);
        let value = mcbench_json::parse(&text).unwrap();
        assert_eq!(mcbench_json::stringify(&value), text);
        let forms = value["forms"].as_object().unwrap();
        let names: Vec<&str> = forms.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "updates/image",
                "updates/text",
                "strict/image",
                "strict/text"
            ]
        );
        assert_eq!(
            mcbench_json::stringify(&value["budgets"]),
            mcbench_json::to_string(&budgets).unwrap()
        );
    }
}
