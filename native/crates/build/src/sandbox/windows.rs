//! Windows: the Linux build child under the Linux sandbox, inside the app's
//! WSL2 distro (the owner's decision of 2026-11-17, docs/native-app.md, The
//! build-program sandbox). No Windows-native confinement code: each child is
//! one `wsl.exe` running, as the distro's user ([`wsl::USER`]), exactly
//! [`super::linux`]'s command (`prlimit --data=... --core=0 -- bwrap ... --
//! <program>`), and the process protocol runs on that `wsl.exe`'s pipes.
//! Inside the distro the child also installs the seccomp filter, as on
//! Linux: it is the Linux program.
//!
//! The program is the Linux x86_64 `mcbench-build-child` (the app ships it
//! beside `mcbench-lite.exe` under that name, without `.exe`:
//! [`crate::child_program`]). On first use in a process it is copied into
//! the distro, as root, to `/usr/lib/mcbench-build/<its sha256>/<its
//! name>`, checked there by `sha256sum` before it is renamed into place, so
//! a file of that name is always those bytes; a second process finds it
//! there. The distro's [`super::linux::HOST_ENTRIES`] are read in the same
//! call and the command is built from them as Linux builds it from its own.
//!
//! The distro is the one the `bash` sandbox imports (`mcbench-sandbox`'s
//! `windows` module); `wsl.exe` starts it when it is stopped. Until it is
//! imported every spawn fails, with `wsl.exe`'s words.
//!
//! What happens across `wsl.exe`: the runner's kill is the kill of
//! `wsl.exe`, which closes the child's session in the distro; bubblewrap
//! runs with `--die-with-parent` under it, so the box and the child end
//! with it. The app's death ends every `wsl.exe` through the app's job
//! object (`wsl::adopt`, Windows only). The time limit is the runner's own wall clock,
//! as on every platform; the memory limit is `prlimit`'s `RLIMIT_DATA`
//! inside the distro.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use super::wsl;
use crate::ChildSandbox;
use crate::process::ChildProcess;

/// Where a child's program is installed inside the distro.
pub const INSTALLED: &str = "/usr/lib/mcbench-build";

/// A program installed in the distro: its path there and the distro's host
/// entries as [`super::linux::args_with`] takes them.
struct Installed {
    inside: String,
    entries: HashMap<String, Vec<String>>,
}

/// The Windows sandbox: the Linux sandbox in the app's WSL2 distro.
pub struct Wsl {
    distro: String,
    /// The programs installed in the distro, for the import count
    /// ([`wsl::imports`]) they were installed under: a new import is a new
    /// disk, and the cache is dropped with it.
    installed: Mutex<(u64, HashMap<PathBuf, Arc<Installed>>)>,
}

impl Default for Wsl {
    fn default() -> Self {
        Self::new()
    }
}

impl Wsl {
    /// The sandbox in the app's distro ([`wsl::distro`]). A distro already
    /// registered for this Windows user (the person's `Lxss` key, read
    /// without starting `wsl.exe`) is ready at once; otherwise children wait
    /// for setup's shell step to import it or to say why it cannot (a key
    /// that cannot be read is left to that step too, which reads it again).
    pub fn new() -> Self {
        let distro = wsl::distro();
        if matches!(wsl::registered(&distro), Ok(Some(_))) {
            wsl::found();
        }
        Wsl {
            distro,
            installed: Mutex::default(),
        }
    }

    /// Where `program` (a Linux program on this machine) is inside the
    /// distro, copied there on its first use in this process.
    pub fn inside(&self, program: &Path) -> std::io::Result<String> {
        self.install(program)
            .map(|installed| installed.inside.clone())
    }

    fn install(&self, program: &Path) -> std::io::Result<Arc<Installed>> {
        let mut cache = self.installed.lock().expect("installed");
        let (imports, installed) = &mut *cache;
        if *imports != wsl::imports() {
            *imports = wsl::imports();
            installed.clear();
        }
        if let Some(done) = installed.get(program) {
            return Ok(done.clone());
        }
        let bytes = std::fs::read(program)
            .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {e}", program.display())))?;
        let sha256 = {
            use sha2::Digest;
            sha2::Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        let name = program
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| std::io::Error::other(format!("{}: no file name", program.display())))?
            .to_string();
        let query = self.root(&[
            "if [ -x \"$0/$1/$2\" ]; then echo have; else echo need; fi; shift 2; for e in \"$@\"; do if [ -L \"$e\" ]; then echo \"link $e $(readlink \"$e\")\"; elif [ -e \"$e\" ]; then echo \"bind $e\"; fi; done",
            INSTALLED,
            &sha256,
            &name,
        ], None)?;
        let mut lines = query.lines();
        if lines.next() == Some("need") {
            self.root(
                &[
                    "set -e; d=$0/$1; mkdir -p \"$d\"; cat > \"$d/.part\"; echo \"$1  $d/.part\" | sha256sum -c -s; chmod 0755 \"$d/.part\"; mv \"$d/.part\" \"$d/$2\"",
                    INSTALLED,
                    &sha256,
                    &name,
                ],
                Some(&bytes),
            )?;
        }
        let mut entries = HashMap::new();
        for line in lines {
            let words: Vec<&str> = line.split(' ').collect();
            let args = match words.as_slice() {
                ["link", file, target] => {
                    vec!["--symlink".into(), target.to_string(), file.to_string()]
                }
                ["bind", file] => vec!["--ro-bind".into(), file.to_string(), file.to_string()],
                _ => {
                    return Err(std::io::Error::other(format!(
                        "the distro {} answered {line:?} for its host entries",
                        self.distro
                    )));
                }
            };
            entries.insert(words[1].to_string(), args);
        }
        let done = Arc::new(Installed {
            inside: format!("{INSTALLED}/{sha256}/{name}"),
            entries,
        });
        installed.insert(program.to_path_buf(), done.clone());
        Ok(done)
    }

    /// Runs busybox's `sh -c script` as root in the distro with `args` (the
    /// script's `$0`, `$1`, ...), the host entries after them, and `input` on
    /// its stdin; its stdout, or an error with its words.
    fn root(&self, script_and_args: &[&str], input: Option<&[u8]>) -> std::io::Result<String> {
        let mut command = wsl::exec(&self.distro, "root").map_err(std::io::Error::other)?;
        command
            .args(["/usr/bin/busybox", "sh", "-c"])
            .args(script_and_args)
            .args(super::linux::HOST_ENTRIES)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        if let Some(input) = input {
            let mut stdin = child.stdin.take().expect("piped stdin");
            stdin.write_all(input)?;
        }
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(std::io::Error::other(format!(
                "installing the build child in the WSL distro {}: {} {}",
                self.distro,
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// The `wsl.exe` command that starts `program` confined, with an empty
    /// environment but `env` (busybox's `env -i`: nothing of WSL's own
    /// environment reaches the child).
    pub fn command(
        &self,
        program: &Path,
        env: &[(&str, &OsStr)],
    ) -> std::io::Result<std::process::Command> {
        let installed = self.install(program)?;
        let args = super::linux::args_with(&installed.inside, &installed.inside, |file| {
            installed.entries.get(file).cloned().unwrap_or_default()
        });
        let mut command = wsl::exec(&self.distro, wsl::USER).map_err(std::io::Error::other)?;
        command.args(["/usr/bin/busybox", "env", "-i"]);
        for (key, value) in env {
            let value = value
                .to_str()
                .ok_or_else(|| std::io::Error::other(format!("the variable {key} is not UTF-8")))?;
            command.arg(format!("{key}={value}"));
        }
        command.arg("/usr/bin/prlimit").args(args);
        Ok(command)
    }
}

impl ChildSandbox for Wsl {
    fn spawn(&self, program: &Path, env: &[(&str, &OsStr)]) -> std::io::Result<ChildProcess> {
        let mut command = tokio::process::Command::from(self.command(program, env)?);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        #[cfg(windows)]
        wsl::adopt(&child)?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        Ok(ChildProcess::new(
            Box::new(child),
            Box::new(stdin),
            Box::new(stdout),
            Box::new(stderr),
        ))
    }

    fn spare_during_build(&self) -> bool {
        // As on Linux: the next process starts once a build's request is
        // written, which hides wsl.exe's start (about 0.2 s warm).
        true
    }

    /// The child installed into the distro (copied and checked as root,
    /// once per import): `wsl.exe` runs, so the runner calls this on the
    /// blocking pool and [`ChildSandbox::spawn`] then finds it installed.
    fn prepare(&self, program: &Path) -> std::io::Result<()> {
        self.install(program).map(drop)
    }

    /// What setup's shell step knows of the distro ([`wsl::state`]): no
    /// `wsl.exe` is started for a child before it is ready.
    fn readiness(&self) -> tokio::sync::watch::Receiver<crate::Readiness> {
        wsl::state()
    }
}
