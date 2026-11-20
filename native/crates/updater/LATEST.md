# `latest.json`

The in-app updater (routes `script` on macOS and Linux, `appimage`, and `direct` on Windows) reads `latest.json` from the address compiled into the build (`MCBENCH_UPDATE_FEED`; for a release, `https://github.com/adonis-singh/mcbench-lite/releases/latest/download/latest.json`; for a staging build, a local address) and downloads the asset it names from the same folder (`releases/latest/download/<name>`). `mcbench-feed sign` writes it and signs every asset with the updater's minisign key (the CI secret `MINISIGN_KEY`, given to it as the file `MCBENCH_SIGNING_KEY_FILE` names). `mcbench-updater` refuses a file with a field this page does not name.

```json
{
  "version": "1.2.0",
  "keys": [
    { "public_key": "RWQ...next key...", "signature": "untrusted comment: ...\nRUQ...\ntrusted comment: mcbench-lite update key\n...\n" }
  ],
  "assets": {
    "linux-x86_64-appimage": {
      "name": "mcbench-lite-x86_64.AppImage",
      "size": 123456789,
      "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
      "signature": "untrusted comment: ...\nRUQ...\ntrusted comment: version:1.2.0\ttarget:linux-x86_64-appimage\tfile:mcbench-lite-x86_64.AppImage\tsize:123456789\tsha256:9f86...0a08\n...\n"
    }
  }
}
```

| Field | What it is |
|---|---|
| `version` | The release's version, `MAJOR.MINOR.PATCH` (three decimal numbers, nothing else) |
| `keys` | The key chain, oldest first: each entry is a public key (`public_key`, the base64 line of `minisign.pub`) and the minisign signature (`signature`, the whole `.minisig` text) of that base64 line's bytes by the key before it. `mcbench-feed sign-key` writes the trusted comment `mcbench-lite update key`; the updater does not read it. The first entry is signed by the first key ever compiled into a released app. Empty while the first key is the only one |
| `assets` | One entry per target, keyed by the target's name below |
| `assets.<target>.name` | The asset's file name in the release, always the target's fixed name below |
| `assets.<target>.size` | Its size in bytes, at most 512 MiB |
| `assets.<target>.sha256` | Its sha256, 64 lower-case hexadecimal digits |
| `assets.<target>.signature` | The asset's minisign signature (prehashed, as `minisign -S` writes it today), the whole `.minisig` text, whose trusted comment is `version:<version>\ttarget:<target>\tfile:<name>\tsize:<size>\tsha256:<sha256>` (tabs between the fields), the same values as the entry and `version` |

| Target | Asset | Holds | What it replaces |
|---|---|---|---|
| `macos-aarch64`, `macos-x86_64` | `mcbench-lite-macos-aarch64.tar.gz`, `mcbench-lite-macos-x86_64.tar.gz` | `mcbench-lite.app` at its root, a bundle of one architecture (the archive `install.sh` installs) | the `.app` bundle, swapped whole (route `script`, the only macOS route) |
| `linux-x86_64`, `linux-aarch64` | `mcbench-lite-linux-x86_64.tar.gz`, `mcbench-lite-linux-aarch64.tar.gz` | one folder `mcbench-lite/` at its root: the payload folder's files and `share/` | the payload folder `~/.local/lib/mcbench-lite/`, swapped whole (route `script`) |
| `linux-x86_64-appimage`, `linux-aarch64-appimage` | `mcbench-lite-x86_64.AppImage`, `mcbench-lite-aarch64.AppImage` | the AppImage | the AppImage file (`APPIMAGE`) |
| `windows-x86_64` | `mcbench-lite-windows-x86_64.tar.gz` | the payload folder's files at its root, without `install-source` and `uninstall.exe` | each installed entry of the payload folder `%LOCALAPPDATA%\Programs\mcbench-lite\` that the archive names, together (route `direct`); the others stay |

The archives are written by `mcbench-feed pack` (with `--as mcbench-lite.app` or `--as mcbench-lite` for the folders), which gives the same bytes for the same files: entries sorted, times 0, owner 0, modes 0755 for folders and executables and 0644 otherwise, symlinks kept, a gzip header with time 0. The install's own `install-source` is carried into the new payload by the swap, so the archives' own one, if any, is never used. The payload's `version` file is part of every archive.

## What the app checks, in order

| Step | Refused when |
|---|---|
| The feed | the build has no compiled-in key and feed (`MCBENCH_UPDATE_KEY_FILE` and `MCBENCH_UPDATE_FEED` unset at build time): such a build never checks and makes no request; a build with only one of them fails |
| `version` | it is older than the running app's (a downgrade); the same version means up to date and nothing more is read |
| `keys` | starting after the entry equal to the compiled-in key (from the first entry when none is), an entry's signature is not by the key before it |
| `assets` | there is no entry for this install's target, or its size is over 512 MiB (refused before anything is downloaded) |
| The signature's key | the asset is not signed by the last key of the chain (or the compiled-in key when the chain adds none) |
| The signed comment | its `target` is another target, or any field differs from the entry and `version` (a tampered `latest.json`) |
| The name | it is not the target's fixed asset name |
| The download | it is longer or shorter than `size`, or its sha256 is not `sha256` (a tampered asset), checked while it streams to `.<name>.update` beside what it replaces; a check whose answer takes over 60 seconds, or a download that receives nothing for 60 seconds, gives up |
| The signature | it does not verify over the downloaded bytes and the signed comment (minisign's global signature covers the trusted comment, so a changed comment fails here) |

The check runs at start and 24 hours of wall-clock time after each, on the updater's own thread. A download that passed every step waits beside the install as `.<name>.update`, with the `latest.json` it was checked against as `.<name>.update.json`.

A release refused by any step above (`latest.json`'s key chain, signed comment, name, size or version, or the asset's size, sha256 or signature) is remembered in `.<name>.update.refused` beside the install: the sha256 of the `latest.json` bytes that led to the refusal and its sentence, written to `update.log` once. Later checks still fetch `latest.json`; while its bytes are the same they download nothing and log nothing, and Settings shows the first refusal. When `latest.json` changes the file is removed and the check goes on as usual. A failure that is not a refusal of the release (no network, a timeout, the server's error, a download that ended early) is not remembered and is tried again at the next check.

At the next start, before the core and any window, the app takes its data folder's one-instance lock, then the install's gate `.<name>.gate` (waiting while another start decides), then tries the presence file `.<name>.lock` exclusively without waiting. Every app, headless run and error screen of the install holds the presence file shared while it runs, taking that hold under the gate, so a swap happens only when nothing runs from the install. Then every step above is made again from the two waiting files without the network. A file that fails is deleted. One that passes is swapped in, and the new program is started in the app's place with the same arguments; when it cannot be started, the previous version is put back, the app starts as it is, and the version is kept in `.<name>.update.held` so no later start takes it until another is offered. With no swap, the app holds the presence file shared before it releases the gate. A swap that fails at any step reverses every step before it, so the old install stays whole and the download waits for the next start; when a reversal fails too, the app shows the sentence as fatal, naming the renames to make by hand. The previous version (`<name>.old`) is removed once the new program's core has started. Failures are written to `update.log` in the app's logs folder.

Where the presence file or the gate cannot be made or opened, or the file system refuses the lock, no hold is possible: the app and every headless run start as they are, nothing is swapped, no check runs, and Settings says why.

Every payload carries a file `version` beside `install-source` (`Contents/Resources/version` on macOS, inside the AppImage's `usr/lib/mcbench-lite/`), one line `MAJOR.MINOR.PATCH`, written by the packaging step; the archives carry it as part of the payload, and on Windows it is an ordinary entry the entry swap replaces. The swap refuses a download whose unpacked `version` is not the release's version. On every start, once its hold is taken, a process compares its own version with the installed payload's `version`, and a process that finds itself loaded from a payload a swap moved aside (`<payload>.old`, or inside `.<payload>.update.d`) counts as replaced too. When the payload is not this program's, the app starts the installed program once (a second time is a fatal sentence); a headless run prints "mcbench-lite was updated on disk while this process was starting, so it stopped before doing anything; start it again." and exits with status 2; the error screen's process, which starts nothing from the payload, holds nothing and goes on. The AppImage is not compared: a process runs wholly from the image it mounted. A missing `version` file is a broken install at the app's start and no hold for a headless run.

A wait for the gate longer than five seconds writes one line to `update.log` and goes on waiting.

On macOS and Linux a folder and the AppImage are swapped by one exchange of the new and the installed path, so a power loss leaves either the old or the new payload in place (the other at the unpack place or `<name>.old`). A file system without the exchange (`EINVAL`, `ENOSYS`, `ENOTSUP`, `EOPNOTSUPP`; for example HFS+, exFAT, NFS and many FUSE mounts) gets two renames instead, kept on purpose so such installs still update; a power loss between them leaves nothing at the install path, and running the installer again repairs it. On Windows the payload's entries are swapped one at a time, each entry's two renames adjacent and `mcbench-lite.exe` last; a power loss between an entry's two renames leaves that entry as `<name>.old` with nothing at `<name>`, and entries before it already new. Running the installer again repairs such an install.

On Windows the new program, once its core runs, sets `DisplayVersion` of the installer's uninstall entry to its own version when the entry's `InstallLocation` is its install folder.

## Open

There is no automatic rollback when a new version starts and then crashes: the previous version is removed once the new core has started, so going back is a reinstall of the earlier release.

The presence file can be deleted while the app runs (an uninstall, a person). The next start then makes a new one, its exclusive try succeeds, and a swap can happen under the running processes. Accepted as a corner case.

A headless run prints the sentence above whenever the payload of a running app is replaced outside the updater (`install.sh` run again, a package or a person copying files) or a swap happens under it because the presence file was deleted: each engine child the app starts after that is the new program's `headless engine` against the app's old version, or the old program on a new payload, finds the versions differ, and exits with status 2, so the running app shows engine failures until it is restarted. Accepted: it refuses a version mix, and a restart of the app ends it.

Two users with different rights on one install: one who cannot create the presence and gate files runs with no hold, and one who can creates them and may swap under the first. Accepted; the installers can create both files to close it.

## Rotating the key

Generate the next key pair (`mcbench-feed keygen <folder>`), print the next key's chain entry signed by the current key (`MCBENCH_SIGNING_KEY_FILE=<current minisign.key> mcbench-feed sign-key <next minisign.pub>`), append it to `keys` of the last release's `latest.json`, compile the next public key into the release, and sign its assets with the next key, copying the chain through (`MCBENCH_SIGNING_KEY_FILE=<next minisign.key> mcbench-feed sign --chain <that latest.json> <release folder> <version> <target>...`; a chain that does not end at the signing key is refused). Keep every entry: an app still on an older key follows the chain from its own key to the newest.
