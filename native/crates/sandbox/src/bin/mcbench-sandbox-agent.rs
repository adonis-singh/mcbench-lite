//! `mcbench-sandbox-agent`: [`mcbench_sandbox::agent::main`] as a program
//! of its own, small and free of the app's GPU and V8 dependencies, built
//! static (musl) for the macOS VM and the WSL2 distro, and the relay inside
//! Codex's box on Linux. `mcbench-lite sandbox-agent` is the same program.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe = std::env::current_exe().expect("the agent's own path");
    mcbench_sandbox::agent::main(vec![exe.into_os_string()], &args)
}
