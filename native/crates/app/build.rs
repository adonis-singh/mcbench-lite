//! Build script of mcbench-lite: test gates.
// The tests of the engine protocol and of the in-process engine's
// equivalence need Minecraft's tables: they run when MCBENCH_TABLES names
// their folder (core/minecraft/1.21.10) at build time, and are ignored with
// the reason otherwise (native/check.sh stays self-contained).
fn main() {
    println!("cargo::rerun-if-env-changed=MCBENCH_TABLES");
    println!("cargo::rustc-check-cfg=cfg(mcbench_tables)");
    if std::env::var_os("MCBENCH_TABLES").is_some() {
        println!("cargo::rustc-cfg=mcbench_tables");
    }
}
