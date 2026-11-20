//! `mcbench-feed`: the release side of the updater (feature `feed`). The
//! commands are `mcbench_updater::feed`'s; a failure prints one sentence and
//! exits with status 1, a wrong command line prints the usage and exits
//! with status 2.

use std::path::{Path, PathBuf};

use mcbench_updater::Target;
use mcbench_updater::feed;

const USAGE: &str = "usage: mcbench-feed keygen <folder>
       mcbench-feed pack [--as <name>] <folder> <out.tar.gz>
       mcbench-feed sign [--chain <latest.json>] <release folder> <version> <target>...
       mcbench-feed sign-key <next minisign.pub>
sign and sign-key read the secret key from the file MCBENCH_SIGNING_KEY_FILE names.";

fn usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2)
}

fn signing_key() -> Result<(minisign::SecretKey, minisign::PublicKey), String> {
    let path = std::env::var_os(feed::SIGNING_KEY_VAR)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{} is not set.", feed::SIGNING_KEY_VAR))?;
    feed::signing_key(Path::new(&path))
}

fn run(args: &[String]) -> Result<(), String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["keygen", folder] => {
            let folder = Path::new(folder);
            let public = feed::keygen(folder)?;
            println!(
                "Wrote {} and {}; the public key is {public}.",
                folder.join(feed::PUBLIC_FILE).display(),
                folder.join(feed::SECRET_FILE).display()
            );
            Ok(())
        }
        ["pack", "--as", root, folder, out] => {
            feed::pack(Path::new(folder), Some(root), Path::new(out))
        }
        ["pack", folder, out] => feed::pack(Path::new(folder), None, Path::new(out)),
        ["sign", "--chain", chain, release, version, targets @ ..] if !targets.is_empty() => {
            sign(release, version, targets, Some(PathBuf::from(chain)))
        }
        ["sign", release, version, targets @ ..] if !targets.is_empty() => {
            sign(release, version, targets, None)
        }
        ["sign-key", next] => {
            let (secret, public) = signing_key()?;
            println!("{}", feed::sign_key(Path::new(next), &secret, &public)?);
            Ok(())
        }
        _ => usage(),
    }
}

fn sign(
    release: &str,
    version: &str,
    targets: &[&str],
    chain: Option<PathBuf>,
) -> Result<(), String> {
    let targets = targets
        .iter()
        .map(|t| Target::from_name(t))
        .collect::<Result<Vec<_>, _>>()?;
    let (secret, public) = signing_key()?;
    let out = feed::sign(
        Path::new(release),
        version,
        &targets,
        &secret,
        &public,
        chain.as_deref(),
    )?;
    println!("Wrote {}.", out.display());
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(sentence) = run(&args) {
        eprintln!("mcbench-feed: {sentence}");
        std::process::exit(1);
    }
}
