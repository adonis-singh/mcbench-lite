//! macOS: `sandbox-exec` with `build-sandbox.js`'s seatbelt profile.
//!
//! The profile is `SEATBELT_PROFILE`'s lines with the child's program
//! (`mcbench-build-child`) where `process.execPath` is (the exec grant and
//! the read of the program and its real path) and without the read grants
//! of the five sources, which the program embeds. It denies everything but reading the system's libraries,
//! devices and time zones, the listing of `/`, metadata, executing the
//! program and writing `/dev/null`: no network, no writes, no other exec. A
//! test holds every line against `server/build-sandbox.js`.

use std::ffi::OsStr;
use std::path::Path;

use super::quoted;
use crate::ChildSandbox;
use crate::process::ChildProcess;

/// The system folders the profile lets the child read
/// (`build-sandbox.js` `SEATBELT_PROFILE`).
pub const READABLE_DIRS: [&str; 5] = [
    "/System",
    "/usr/lib",
    "/usr/share",
    "/private/var/db/timezone",
    "/dev",
];

/// `build-sandbox.js`'s seatbelt profile for `program` (its path as given
/// and its real path); fails when `program` does not exist.
pub fn profile(program: &Path) -> std::io::Result<String> {
    let given = program.to_str().expect("the child's path is UTF-8");
    let real = std::fs::canonicalize(program)?;
    let real = real.to_str().expect("the child's path is UTF-8");
    let mut files = vec![given];
    if real != given {
        files.push(real);
    }
    let readable: Vec<String> = READABLE_DIRS
        .iter()
        .map(|dir| format!("(subpath {})", quoted(dir)))
        .chain(
            files
                .iter()
                .map(|file| format!("(literal {})", quoted(file))),
        )
        .collect();
    Ok([
        "(version 1)".to_string(),
        "(deny default)".to_string(),
        format!("(allow process-exec (literal {}))", quoted(given)),
        "(allow file-read-metadata)".to_string(),
        format!("(allow file-read* (literal \"/\") {})", readable.join(" ")),
        "(allow file-write* (literal \"/dev/null\"))".to_string(),
        "(allow sysctl-read)".to_string(),
        "(deny network*)".to_string(),
    ]
    .join("\n"))
}

/// `sandbox-exec -p <profile> <program>`.
pub fn command(program: &Path) -> std::io::Result<std::process::Command> {
    let mut command = mcbench_spawn::command("/usr/bin/sandbox-exec");
    command.arg("-p").arg(profile(program)?).arg(program);
    Ok(command)
}

/// The child under `sandbox-exec` ([`command`]).
pub struct Seatbelt;

impl ChildSandbox for Seatbelt {
    fn spawn(&self, program: &Path, env: &[(&str, &OsStr)]) -> std::io::Result<ChildProcess> {
        ChildProcess::start(command(program)?, env)
    }

    fn spare_during_build(&self) -> bool {
        // A process starting during a build slows that build on macOS
        // (build-sandbox.js: about 2 ms on a two-million-block world).
        false
    }
}
