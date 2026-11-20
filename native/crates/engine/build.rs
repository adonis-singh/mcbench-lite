//! Build script of mcbench-engine: test gates.
// The tests that need Minecraft's tables (MCBENCH_TABLES, the folder
// core/minecraft/1.21.10), the program engine children run
// (MCBENCH_ENGINE_PROGRAM, a built mcbench-lite) or Node (MCBENCH_DIFF_NODE,
// the node program) are compiled to run when the variable is set at build
// time, and are ignored with the reason otherwise (native/check.sh stays
// self-contained).
fn main() {
    for (variable, cfg) in [
        ("MCBENCH_TABLES", "mcbench_tables"),
        ("MCBENCH_ENGINE_PROGRAM", "mcbench_program"),
        ("MCBENCH_DIFF_NODE", "mcbench_node"),
    ] {
        println!("cargo::rerun-if-env-changed={variable}");
        println!("cargo::rustc-check-cfg=cfg({cfg})");
        if std::env::var_os(variable).is_some() {
            println!("cargo::rustc-cfg={cfg}");
        }
    }
}
