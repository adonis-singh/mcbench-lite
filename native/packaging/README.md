# native/packaging

Stage five's scripts: the release builds, the installers and packages, the bash sandbox's assets, the source bundle and the assembly of a release (docs/native-app.md, Packaging and distribution, and Stage five). The workflows that run them on GitHub are proposals under `native/ci/` (`native/ci/README.md`). No script here uploads, tags, publishes or submits anything; every public step is the owner's and is listed at the end.

An install is one payload folder, and the app finds every shipped file beside its own executable. The sandbox's root filesystem and guest base are in no payload: the app downloads them at first start by `native/sandbox-assets.lock`.

## The scripts

| Script | Takes | Makes |
|---|---|---|
| `set-version.sh <MAJOR.MINOR.PATCH>` | the version | the version in `native/Cargo.toml` (`[workspace.package]`) and `native/build-child/Cargo.toml`, and the entries of the workspace's own crates in both `Cargo.lock` files; no other lock line, no network |
| `build/linux.sh <repository> <arch> <out>` | `MCBENCH_DATA_BUNDLE`; `MCBENCH_UPDATE_KEY_FILE` and `MCBENCH_UPDATE_FEED` when the build has the updater (a release always); `CARGO_BUILD_JOBS`; Docker on an `<arch>` machine | the Linux payload folder: `mcbench-lite`, `mcbench-build-child`, `mcbench-sandbox-agent`, built in the pinned Debian 10 image, and on x86_64 the software Vulkan driver `libvk_swiftshader.so` with its licences in `licenses/swiftshader/` (`build/swiftshader.sh`) |
| `build/swiftshader.sh <cache> <out>` | Docker on an x86_64 machine; `CARGO_BUILD_JOBS` | `libvk_swiftshader.so` (SwiftShader built from its pinned commit in AlmaLinux 8, refused unless its sha256 is the pin's) and `licenses/swiftshader/`; `<cache>` keeps the source and the checked driver, so a later run builds nothing (`linux/README.md`, The software Vulkan driver) |
| `build/macos.sh <repository> <target> <out>` | the same three variables and `CARGO_TARGET_DIR`; a Mac | one architecture's `mcbench-lite.app`, signed ad hoc, and `mcbench-lite-macos-<arch>.tar.gz` |
| `build/windows.ps1 -Repository -LinuxFiles -Out` | the same three variables; an x64 Windows machine; `-LinuxFiles` holding the Linux x86_64 `mcbench-build-child` | the Windows x64 payload folder (Windows on ARM is not in the first release) |
| `linux/archive.sh`, `deb.sh`, `rpm.sh`, `appimage.sh` `<payload> <version> <arch> <out>` | a Linux payload; `MCBENCH_LITE_SIGNING_KEY` (the armoured OpenPGP secret key) for `deb.sh` and `rpm.sh`; `MCBENCH_LITE_RELEASES` and `MCBENCH_LITE_SITE`, which the packages' repository files name | `mcbench-lite-linux-<arch>.tar.gz`, `mcbench-lite_<amd64\|arm64>.deb`, `mcbench-lite.<arch>.rpm`, `mcbench-lite-<arch>.AppImage` |
| `linux/apt-repo.sh <release>`, `dnf-repo.sh <release> <version> <site>`, `aur.sh <release> <version> <out> <arch>...` | a release folder; `MCBENCH_LITE_SIGNING_KEY` for the two repositories; `MCBENCH_LITE_RELEASES` for the versioned addresses the dnf metadata and the `PKGBUILD` name | the flat apt repository among the assets, `<site>/rpm/repodata/`, `PKGBUILD` and `.SRCINFO` (`release.sh` runs all three; `native/packaging/linux/README.md`) |
| `windows/installer.sh <payload> <version> <out>` | the Windows payload; Docker on Linux or NSIS on Windows | `mcbench-lite-setup-x64.exe` |
| `windows/winget.sh <release> <version> <out>` | the x64 installer | the winget manifest's three files (`release.sh` runs it) |
| `sandbox-assets.sh build\|check\|fetch` | `sandbox-assets.md` | the sandbox's four assets, the lock, the package lists; the rebuild-and-compare check |
| `sources.sh [--check] <lists> <lock> <out>` | the lists and lock of a `sandbox-assets.sh build` | the source bundle folder of every Debian package in the guest base and the root filesystem (798 MB for today's assets) |
| `sandbox-release.sh <assets> <sources> <out>` | the `<out>` of `sandbox-assets.sh build` and the `<out>` of `sources.sh` | the folder of the sandbox's own release `sandbox-assets-<hash>`: the four assets, pasta's source, the lock, `sandbox-sources.tar` (an uncompressed, deterministic tar of the source bundle) and `SHA256SUMS` |
| `release.sh <inputs> <version> <out> [--base <address> --site <address>] [--chain <latest.json>] [--only <platform>...]` | the assets under their fixed names anywhere in `<inputs>`; `MCBENCH_SIGNING_KEY_FILE`; `MCBENCH_REPO_KEY` (a key id in the caller's GnuPG home) when a Linux platform is in it; `mcbench-feed` on `PATH`; Docker and gpg for Linux | `<out>/release/` and `<out>/site/`, below |
| `test/linux/staging-key.sh`, `stage.sh`, `serve.sh`, `machine.sh` | Docker (`native/packaging/linux/README.md`, The clean-machine tests) | a staging OpenPGP key; every Linux package of a payload staged with the staging server's addresses; the server; one route's install, start, upgrade and removal on a fresh container |
| `test/macos/release-install.sh <release> <scratch> <port>` | `release.sh`'s `<out>/release`; a Mac | `install.sh --no-open` from the release served on 127.0.0.1 into a scratch home folder; the bundle's signature, `install-source` and no quarantine attribute checked; `--uninstall`; the home folder's listing compared with the one before |
| `test/windows/vm-test.sh`, `guest/vm-test.ps1` | the Windows VM on adipc, or a Windows runner for the `install` and `uninstall` steps | `native/packaging/test/windows/README.md` |

`mcbench-feed` is the updater crate's release program (`cargo build -p mcbench-updater --features feed --bin mcbench-feed`): `keygen <folder>` writes a minisign key pair, `pack` writes the updater's deterministic archives, `sign` writes `latest.json` (`native/crates/updater/LATEST.md`).

## What `release.sh` makes

The table at the top of `release.sh` is the one list of the assets a release requires. It fails naming every asset it does not find, and names any asset it finds twice.

| Platform | Assets | Feed targets |
|---|---|---|
| `macos-aarch64`, `macos-x86_64` | `mcbench-lite-macos-<arch>.tar.gz` | `macos-<arch>` |
| `linux-x86_64`, `linux-aarch64` | `mcbench-lite-linux-<arch>.tar.gz`, `mcbench-lite-<arch>.AppImage`, `mcbench-lite_<amd64\|arm64>.deb`, `mcbench-lite.<arch>.rpm` | `linux-<arch>`, `linux-<arch>-appimage` |
| `windows-x86_64` | `mcbench-lite-setup-x64.exe`, `mcbench-lite-windows-x86_64.tar.gz` | `windows-x86_64` |

| Path | What it is |
|---|---|
| `release/` (the files directly in it) | the release's assets: every asset above, the flat apt repository (`Packages`, `Release`, `InRelease`, `Release.gpg`), `latest.json` signed by `mcbench-feed sign`, and `SHA256SUMS` over every other file |
| `release/aur/` | `PKGBUILD` and `.SRCINFO` of `mcbench-lite-bin`; not assets |
| `release/winget/` | the winget manifest of `adonis-singh.mcbench-lite`; not assets; made when `windows-x86_64` is in the release |
| `site/` | `native/site/` with the sources link's marked `href` filled from `native/sandbox-assets.lock`'s url line (its `releases/tag/` page), its links to the release and to the site set to `--base`'s and `--site`'s, and the dnf metadata in `rpm/repodata/` |

`--base` is the address of the repository the release is served from, in GitHub's shape: fixed links are `<base>/releases/latest/download/<name>` and versioned links `<base>/releases/download/v<version>/<name>`. `--site` is the address the site is served from. They are given together or not at all: the defaults are `https://github.com/adonis-singh/mcbench-lite` and `https://adonis-singh.github.io/mcbench-lite`, and a staging release names both of its own, because a staging site cannot be derived from a staging releases address. The Linux packages given to `release.sh` must have been made with the same two addresses (`MCBENCH_LITE_RELEASES=<base>/releases/latest/download`, `MCBENCH_LITE_SITE=<site>`), since the `.deb`'s apt source and the `.rpm`'s repo file name them. While the lock's url is `not-published-yet` the link is that word (a staging release, or a trial run of the release workflow); the release workflow's `version` job refuses that lock for a release. `--only` makes a partial release of the named platforms for a local test; `latest.json` then names only their targets. `--chain` names the previous release's `latest.json` after a key rotation, and its key chain is copied into the new one (`native/crates/updater/LATEST.md`, Rotating the key).

## A release, from tag to draft

| Step | Who | What |
|---|---|---|
| 1 | the owner | Publish the sandbox assets' release if the lock changed (the steps below), and commit its lock |
| 2 | the session holding the main checkout | `native/packaging/set-version.sh <version>`, commit, tag `v<version>` (with the owner's word) |
| 3 | `release.yml`, job `version` | fails unless `native/packaging/update-key.pub` exists, the tag equals the workspace's version, and the sandbox lock's url is published |
| 4 | jobs `data`, `sandbox`, `feed` | the Minecraft data bundle regenerated and compared with `native/minecraft-data.lock`; the guest bases rebuilt and compared with `native/sandbox-assets.lock`; `mcbench-feed` built |
| 5 | jobs `build-macos`, `build-linux`, `build-windows` | one build per target on its own runner, each compiling in the data bundle, `update-key.pub` and the feed's address |
| 6 | jobs `package-linux`, `package-windows` | the Linux packages per architecture; the Windows installer and the updater's archive |
| 7 | job `assemble` | `release.sh` with the two signing keys from the secrets |
| 8 | jobs `test-linux`, `test-windows`, `test-macos` | Linux: `test/linux/stage.sh` from the x86_64 payload and `machine.sh` for apt, dnf, aur, script and AppImage; Windows: the assembled installer's silent install and removal with `guest/vm-test.ps1`; macOS: `test/macos/release-install.sh` on the assembled release |
| 9 | job `draft` | a draft GitHub release holding the files of `release/` |
| 10 | the owner | reads the draft and publishes it; then starts `pages.yml` (or it starts itself on the publish), `publish-aur.yml`, and opens the winget pull request |
| 11 | the session holding the main checkout | adds the `preferences.json` the released build writes to `native/crates/ui/tests/fixtures/preferences/`, named by its version (`2.0.0.json`): start the release build once with `MCBENCH_DATA_DIR` and `HOME` naming scratch folders (on the Mac in the guest, as every windowed start), move the window, quit, and copy the file from the scratch data folder. The interface's test reads every file in that folder, so the next version's build fails if it cannot read what this release wrote (that folder's README) |

## A staging release on the local machines

A staging release is `release.sh` with `--base` and `--site` naming local addresses, a staging minisign key and a staging OpenPGP key, and is what the local install and update tests serve. The keys are files in a scratch folder and are never committed.

```
mcbench-feed keygen <scratch>/keys                         # minisign.pub and minisign.key
export GNUPGHOME=<scratch>/gnupg                           # a scratch GnuPG home with a key without passphrase
gpg --batch --passphrase '' --quick-gen-key 'mcbench-lite staging <staging@mcbench-lite.invalid>' ed25519 sign never
MCBENCH_SIGNING_KEY_FILE=<scratch>/keys/minisign.key MCBENCH_REPO_KEY=<its fingerprint> \
  native/packaging/release.sh <inputs> 1.0.0 <scratch>/out \
  --base http://mcbn-s5-linux-serve:8000 --site http://mcbn-s5-linux-serve:8001 [--only ...]
```

The two addresses above are the Linux staging server's (`native/packaging/test/linux/env.sh`). Every part of a staging release names the same addresses:

| Part | Made with |
|---|---|
| The app's builds (for an update test) | `MCBENCH_UPDATE_KEY_FILE=<scratch>/keys/minisign.pub` and `MCBENCH_UPDATE_FEED` set to where the test serves `latest.json`: `<base>/releases/latest/download/latest.json` in GitHub's shape, or `http://127.0.0.1:<port>/latest.json` for a flat folder |
| The Linux packages | `MCBENCH_LITE_RELEASES=<base>/releases/latest/download`, `MCBENCH_LITE_SITE=<site>` and the exported staging OpenPGP key as `MCBENCH_LITE_SIGNING_KEY` |
| `release.sh` | the same `--base` and `--site`, `MCBENCH_SIGNING_KEY_FILE`, `MCBENCH_REPO_KEY` |

A second version for an update test is made by running `set-version.sh` with the next patch version (2.0.0 gives 2.0.1) on a scratch copy of the tree and building from it. On a Linux machine with a desktop session in use, run gpg inside `bwrap --dev-bind / / --tmpfs /run/user/1000 --unshare-pid --die-with-parent`, so its agent's sockets stay out of that session's runtime folder (/run/user/1000 is the first user's).

To serve it in GitHub's shape, copy the files of `<out>/release/` to `<staging>/releases/download/v<version>/`, write `v<version>` to `<staging>/releases/LATEST`, and copy `<out>/site/` to `<staging>/site/`, which is the layout `test/linux/server.py` serves. Each platform's install is pointed at it as follows.

| Platform | How the install reads the staging release |
|---|---|
| Linux | `test/linux/serve.sh start <staging>` serves it from the container `mcbn-s5-linux-serve` (releases on port 8000, the site on 8001); `test/linux/machine.sh <system> <route> <staging> <version> [<upgrade version>] <results>` installs it on a clean container. `test/linux/stage.sh` makes such a staging folder straight from a payload, without `release.sh` |
| macOS | `test/macos/release-install.sh <out>/release <scratch> <port in 3190-3199>` serves the release folder flat (install.sh takes the folder that holds the assets) and installs into a scratch home folder |
| Windows | `test/windows/vm-test.sh` with staging releases A and B served to the VM at `http://10.0.2.2:3191` (`native/packaging/test/windows/README.md`) |

## Sizes and times

The measured download sizes, install times, start times and removal times per route are in `docs/native-app.md`, section "Stage five's speed rows, as measured so far". They are container and stand-in timings, not speed-table rows yet.

The software Vulkan driver of the Linux x86_64 payload (`linux/README.md`, The software Vulkan driver) adds 1.77 MB to the archive, 1.27 MB to the `.deb`, 1.44 MB to the `.rpm` and 1.59 MB to the AppImage, and 4.68 MB to the unpacked payload folder; aarch64 has no driver.

## The sandbox assets' release

`sandbox-assets.sh build <repository> <assets> https://github.com/adonis-singh/mcbench-lite/releases/download` makes the assets and a lock whose url line names their release; `sources.sh <assets>/lists <assets>/release/sandbox-assets.lock <sources>` makes the source bundle; `sandbox-release.sh <assets> <sources> <out>` assembles the release folder and prints its name `sandbox-assets-<hash>`. The owner creates that release, uploads every file of `<out>`, and commits `<out>/sandbox-assets.lock` as `native/sandbox-assets.lock`. An app release cannot be cut before that lock is committed: the app downloads its shell's files by it, and the site's sources link points at that release.

## The owner's secrets and one-time steps

| Step | What it takes |
|---|---|
| The updater's minisign key pair | `mcbench-feed keygen <offline folder>`; commit `minisign.pub` as `native/packaging/update-key.pub`; keep `minisign.key` offline and add its whole text as the secret `MINISIGN_KEY`. Until `update-key.pub` exists, the release workflow fails at its first step |
| The OpenPGP key of the apt and dnf repositories | an ed25519 signing key without passphrase (`gpg --quick-gen-key 'mcbench-lite <address>' ed25519 sign never`); add `gpg --armor --export-secret-keys <id>` as the secret `REPO_GPG_KEY`; keep a copy offline. The packages carry its public part, so a new key reaches users only with a new package |
| The workflows | copy the proposals of `native/ci/` to `.github/workflows/` (`native/ci/README.md`) |
| GitHub Pages | Settings, Pages, Source: GitHub Actions |
| The AUR | an AUR account with an SSH key; the private key as the secret `AUR_SSH_KEY`; the first push makes the package `mcbench-lite-bin` |
| winget | the pull request to `microsoft/winget-pkgs` with the manifest from `release/winget/` (`native/ci/README.md`) |
| The sandbox assets | publish the `sandbox-assets-<hash>` release and commit its lock, as above |
| The first tag | `v<version>` on a commit whose version is that version, then reading and publishing the draft |

## What is not built

| Part | State |
|---|---|
| The rollback build (the extractor run on the user's machine with a bundled Java runtime; `docs/native-app.md`, The rollback build) | No cargo feature and no script exist; the workflow's `rollback` job says "not built yet" while `native/packaging/build/rollback.sh` does not exist |
| Windows on ARM | Not in the first release: the app's WSL distro and the Linux build child are x86_64 only |
| The aarch64 Linux build | `build/linux.sh` for aarch64 has never been run on a local machine; only the workflow's `ubuntu-24.04-arm` jobs would build and package it |
| Automatic rollback of a bad update | The updater checks a download before swapping and keeps the old install when a swap fails, but nothing returns to the previous version when a new version that passed its checks fails to start |

## Owed before the first publish

| Part | State |
|---|---|
| What the profile is worth on Windows | Not measured yet: `pgo-replay.js --times` on the pinned build against a `-NoProfile` build in the VM |

## What no script does

Nothing here pushes, tags, creates or publishes a release, uploads an asset, deploys Pages, pushes to the AUR, opens a winget pull request, or writes under `.github/`. The release workflow's last job makes a draft that only the owner can publish, and `publish-aur.yml` runs only when he starts it.
