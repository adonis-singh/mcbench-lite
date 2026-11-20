//! The child's confinement per platform: `build-sandbox.js`'s launchers with
//! the child's program (`mcbench-build-child`) in place of `node` and its
//! five sources (which the program embeds). Every module compiles on every platform; [`platform`]
//! picks the one of the platform the app runs on.

pub mod linux;
pub mod macos;
pub mod windows;
pub mod wsl;

use crate::ChildSandbox;

/// The sandbox of the platform the app runs on: `sandbox-exec` on macOS,
/// `prlimit` and bubblewrap on Linux, the same Linux sandbox inside the
/// app's WSL2 distro on Windows. This is the one place the sandbox is
/// chosen: a Windows-native sandbox would replace the Windows line here and
/// nothing else.
pub fn platform() -> Result<Box<dyn ChildSandbox>, String> {
    #[cfg(target_os = "macos")]
    return Ok(Box::new(macos::Seatbelt));
    #[cfg(target_os = "linux")]
    return Ok(Box::new(linux::Bubblewrap));
    #[cfg(windows)]
    return Ok(Box::new(windows::Wsl::new()));
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    return Err(format!(
        "Build programs run only on Linux, macOS or Windows, not {}",
        std::env::consts::OS
    ));
}

/// `JSON.stringify` of a path, as `build-sandbox.js` quotes paths in the
/// seatbelt profile.
pub(crate) fn quoted(path: &str) -> String {
    serde_json::to_string(path).expect("a string serialises")
}

#[cfg(test)]
mod tests {
    /// `server/build-sandbox.js`, whose launchers these modules port.
    const BUILD_SANDBOX_JS: &str = include_str!("../../../../../server/build-sandbox.js");

    /// The text of `build-sandbox.js` from `start` to the next `end`.
    fn block(start: &str, end: &str) -> &'static str {
        let from = BUILD_SANDBOX_JS
            .find(start)
            .expect("the block is in build-sandbox.js");
        let to = from + BUILD_SANDBOX_JS[from..].find(end).expect("the block ends");
        &BUILD_SANDBOX_JS[from..to]
    }

    #[test]
    fn the_seatbelt_profile_is_build_sandbox_js_s_line_for_line() {
        let js = block("const SEATBELT_PROFILE = [", "].join('\\n');");
        for line in [
            "'(version 1)',",
            "'(deny default)',",
            "`(allow process-exec (literal ${JSON.stringify(process.execPath)}))`,",
            "'(allow file-read-metadata)',",
            "`(allow file-read* (literal \"/\") ${[",
            "...['/System', '/usr/lib', '/usr/share', '/private/var/db/timezone', '/dev'].map((dir) => `(subpath ${JSON.stringify(dir)})`),",
            "...[...new Set([process.execPath, fs.realpathSync(process.execPath)])].map((file) => `(literal ${JSON.stringify(file)})`),",
            "...SOURCES.map((file) => `(literal ${JSON.stringify(file)})`),",
            "'(allow file-write* (literal \"/dev/null\"))',",
            "'(allow sysctl-read)',",
            "'(deny network*)',",
        ] {
            assert!(
                js.contains(line),
                "build-sandbox.js's profile no longer has {line}"
            );
        }
        assert_eq!(
            js.lines().count(),
            13,
            "build-sandbox.js's profile changed:\n{js}"
        );
        let program = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        let profile = super::macos::profile(&program).unwrap();
        let lines: Vec<&str> = profile.lines().collect();
        assert_eq!(lines.len(), 8);
        let literal = super::quoted(program.to_str().unwrap());
        assert_eq!(
            lines[2],
            format!("(allow process-exec (literal {literal}))")
        );
        assert_eq!(
            lines[4],
            format!(
                "(allow file-read* (literal \"/\") (subpath \"/System\") (subpath \"/usr/lib\") (subpath \"/usr/share\") (subpath \"/private/var/db/timezone\") (subpath \"/dev\") (literal {literal}))"
            )
        );
    }

    #[test]
    fn the_linux_arguments_are_build_sandbox_js_s() {
        let js = block("linux: () => ({", "spareDuringBuild: true,");
        for piece in [
            "command: '/usr/bin/prlimit',",
            "args: [`--data=${DATA_LIMIT_BYTES}`, '--core=0', '--', '/usr/bin/bwrap',",
            "'--unshare-user', '--unshare-ipc', '--unshare-pid', '--unshare-net', '--unshare-uts', '--unshare-cgroup',",
            "'--die-with-parent', '--new-session', '--as-pid-1',",
            "'--ro-bind', '/usr', '/usr',",
            "...['/lib', '/lib64', '/bin', '/sbin', '/etc/ld.so.cache', '/etc/localtime'].flatMap(hostEntry),",
            "'--ro-bind', fs.realpathSync(process.execPath), process.execPath,",
            "...SOURCES.flatMap((file) => ['--ro-bind', file, file]),",
            "'--proc', '/proc', '--dev', '/dev', '--remount-ro', '/dev', '--remount-ro', '/', '--chdir', '/',",
            "'--', ...NODE_ARGS],",
        ] {
            assert!(
                js.contains(piece),
                "build-sandbox.js's Linux launcher no longer has {piece}"
            );
        }
        assert!(
            BUILD_SANDBOX_JS
                .contains("const DATA_LIMIT_BYTES = (HEAP_LIMIT_MB + 2048) * 1024 ** 2;")
        );
        assert!(BUILD_SANDBOX_JS.contains("process.env.MCBENCH_BUILD_HEAP_MB || 2048"));
        assert!(BUILD_SANDBOX_JS.contains("darwin: () => ({ command: '/usr/bin/sandbox-exec', args: ['-p', SEATBELT_PROFILE, ...NODE_ARGS], spareDuringBuild: false }),"));
        let program = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        let args = super::linux::args(&program).unwrap();
        assert_eq!(args[0], format!("--data={}", 4096u64 * 1024 * 1024));
        assert_eq!(&args[args.len() - 2..], ["--", program.to_str().unwrap()]);
    }

    /// With `MCBENCH_BUILD_NODE=<node>`: the command `build-sandbox.js`
    /// builds in that Node on this platform, with Node and its sources
    /// replaced by this binary, is exactly this module's.
    #[test]
    #[ignore = "needs MCBENCH_BUILD_NODE=<node>"]
    fn the_command_is_build_sandbox_js_s_with_the_binary_for_node() {
        let node =
            std::env::var("MCBENCH_BUILD_NODE").expect("MCBENCH_BUILD_NODE names a Node binary");
        let server = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../server");
        let server = std::fs::canonicalize(server).unwrap();
        let program = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        let output = std::process::Command::new(node)
            .args([
                "-e",
                NODE_COMMAND,
                server.to_str().unwrap(),
                program.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected: Vec<String> = serde_json::from_slice(&output.stdout).unwrap();
        #[cfg(target_os = "macos")]
        let command = super::macos::command(&program).unwrap();
        #[cfg(not(target_os = "macos"))]
        let command = super::linux::command(&program).unwrap();
        let actual: Vec<String> = std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|arg| arg.to_str().unwrap().to_string())
            .collect();
        assert_eq!(actual, expected);
    }

    /// Prints `build-sandbox.js`'s command on this platform with the binary
    /// in place of Node and without the five sources.
    const NODE_COMMAND: &str = r#"
const fs = require('fs'); const path = require('path');
const [server, program] = process.argv.slice(1);
const { COMMAND, ARGS } = require(path.join(server, 'build-sandbox.js'))._internal;
const SOURCES = ['build-child.js', 'mc-dsl.js', 'world.js', 'minecraft-dimension.js', 'build-transfer.js'].map((n) => path.join(server, n));
const real = fs.realpathSync(program);
const lit = (f) => ` (literal ${JSON.stringify(f)})`;
let out;
if (process.platform === 'darwin') {
  let p = ARGS[1];
  for (const s of SOURCES) p = p.split(lit(s)).join('');
  const dev = '(subpath "/dev")';
  p = p.replace(dev + [...new Set([process.execPath, fs.realpathSync(process.execPath)])].map(lit).join(''), dev + [...new Set([program, real])].map(lit).join(''));
  p = p.replace(`(allow process-exec (literal ${JSON.stringify(process.execPath)}))`, `(allow process-exec (literal ${JSON.stringify(program)}))`);
  out = [COMMAND, '-p', p, program];
} else {
  const cut = ARGS.lastIndexOf('--');
  const head = [];
  for (let i = 0; i < cut; i += 1) {
    if (ARGS[i] === '--ro-bind' && SOURCES.includes(ARGS[i + 1])) { i += 2; continue; }
    if (ARGS[i] === '--ro-bind' && ARGS[i + 2] === process.execPath) { head.push('--ro-bind', real, program); i += 2; continue; }
    head.push(ARGS[i]);
  }
  out = [COMMAND, ...head, '--', program];
}
process.stdout.write(JSON.stringify(out));
process.exit(0);
"#;
}
