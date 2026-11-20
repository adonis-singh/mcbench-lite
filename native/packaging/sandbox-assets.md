# The bash sandbox's release assets

The app's `bash` tool runs in one pinned Linux root filesystem under bubblewrap. On Linux it runs on the host's kernel; on macOS it runs inside the app's own small Linux VM, and on Windows inside a private WSL2 distro, and both of those start from the guest base. These four files are built once, pinned by `native/sandbox-assets.lock`, and published as a release of their own that app releases point at.

| File | Recipe (owned by the sandbox crate) | What it is | Where the app takes it from |
|---|---|---|---|
| `rootfs-x86_64.tar.zst`, `rootfs-aarch64.tar.zst` | `native/crates/sandbox/rootfs/build.sh OUT amd64\|arm64` | The root filesystem the boxes run in, from `sandbox/Dockerfile` (Python, NumPy, Pillow, ImageMagick, jq, ripgrep, iptables), with a static pasta beside it | Downloaded at first start into the data folder's `sandbox/`, for the machine's architecture |
| `guest-base-x86_64.tar.zst`, `guest-base-aarch64.tar.zst` | `native/crates/sandbox/guest/build.sh OUT x86_64\|aarch64` | Debian 13's cloud kernel, an initramfs and the same userland as `userland.tar` (busybox, bubblewrap, the static `mcbench-sandbox-agent`), with a `manifest.json` (`native/crates/sandbox/GUEST.md`) | Shipped in the payload: both in the macOS app's `Contents/Resources/`, the x86_64 one beside `mcbench-lite.exe` on Windows; Linux needs none |

The release also holds `passt-<version>.tar.gz`, the source of the pasta inside each root filesystem (GPL-2.0-or-later), the lock itself, and `SHA256SUMS` over every other file. The sources of the Debian packages in both kinds of asset go into the source bundle that `native/packaging/sources.sh` builds from the lists described below.

## Building

```
native/packaging/sandbox-assets.sh build <repository> <out> [<releases address>]
native/packaging/sandbox-assets.sh build --only guest-base <repository> <out> [<releases address>]
native/packaging/sandbox-assets.sh build --only rootfs <repository> <out> [<releases address>]
```

`build` runs both recipes for both architectures, the guest bases first. It copies each recipe's asset into `<out>/release/` under the fixed name above, byte for byte (the recipes name them `mcbench-sandbox-guest-<arch>.tar.zst` and `mcbench-sandbox-rootfs-<amd64|arm64>.tar.zst`; `amd64` is `x86_64` and `arm64` is `aarch64`). It then writes `sandbox-assets.lock` and `SHA256SUMS` beside them, writes the package lists to `<out>/lists/`, and prints the lock and the release's name. The recipes' own output folders stay in `<out>/work/guest/` and `<out>/work/rootfs/`, so a second build into the same `<out>` reuses the fetched Debian packages and pasta's tarball. The script refuses an `<out>` whose `release/` already exists.

The guest recipe builds the static agent with cargo for both musl targets. The caller gives `RUSTUP_HOME`, `CARGO_HOME`, `CARGO_TARGET_DIR` and a `PATH` with rustup on it, and the toolchain 1.99.1 must have `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` added. The root filesystem recipe needs Docker, with qemu's binfmt handler for arm64, and the `<repository>` must hold `sandbox/Dockerfile`. The script runs it with `MCBENCH_DOCKER_PREFIX=mcbn-s5-`, so every image and container it makes is named `mcbn-s5-*` and is removed by id when it ends; `<out>/work/rootfs/docker-names.txt` lists them.

`--only guest-base` builds the two guest bases and takes the two root filesystem lines from the tracked `native/sandbox-assets.lock`. With no tracked lock it writes the two guest-base lines and the url line only: that is the lock's state until the first root filesystem build, and the app's reader gets its two `rootfs` lines then. `--only rootfs` builds the two root filesystems and takes the two guest-base lines from the tracked lock, and fails when there is none. Either way `release/` holds only the files of the kind that was built.

## The lock

`native/sandbox-assets.lock` is tab-separated, with `#` comment lines, and is never edited by hand: it is `<out>/release/sandbox-assets.lock` from a build, copied over the tracked one.

```
rootfs	x86_64	rootfs-x86_64.tar.zst	<sha256>	<size in bytes>
rootfs	aarch64	rootfs-aarch64.tar.zst	<sha256>	<size>
guest-base	x86_64	guest-base-x86_64.tar.zst	<sha256>	<size>
guest-base	aarch64	guest-base-aarch64.tar.zst	<sha256>	<size>
url	<address of the folder holding the four files>
```

The app compiles the lock in (`mcbench_setup::shell::files`). It checks the guest base it finds in its payload against the lock's sha256, and downloads the root filesystem for its architecture from `<url>/<file>` into the data folder when it is missing or its sha256 differs, then checks it.

## The release and its name

The release is named `sandbox-assets-<16 hex digits>`, the first 16 hexadecimal digits of the sha256 of the lock's lines above its `url` line (its comments and asset lines). The engine's release is named by content in the same way (`engine-<16 hex digits>`). The `url` line is left out of the hash because it holds the name. When `build` is given the releases' address, for GitHub `https://github.com/adonis-singh/mcbench-lite/releases/download`, the url line is that address followed by `/sandbox-assets-<16 hex digits>`; without it the url line is `not-published-yet`. The name depends only on the assets, so a lock built with `not-published-yet` and one built with the address name the same release.

Publishing is the owner's step, done by the session that holds the main checkout with his word each time. He creates a release tagged with the printed name, uploads every file of `<out>/release/` to it, and commits that lock as `native/sandbox-assets.lock`. A release is never changed after it is published. A new lock gives a new name and so a new release. That new release must hold all four files, so after a `--only` build the other kind's two files are fetched from the previous release (`fetch`, run with the previous tracked lock) and uploaded beside the new ones.

## Fetching

```
native/packaging/sandbox-assets.sh fetch <repository> <out>
```

`fetch` downloads every asset the tracked lock names from its `url` into `<out>` and checks each against the lock's sha256 and size. A file that does not match is left as `<file>.part` and the command fails. The app's release build uses it to get the two guest bases for the macOS and Windows payloads. While the lock's url is `not-published-yet` it fails with one sentence saying the assets have not been published.

## Checking, and when the lock must be regenerated

```
native/packaging/sandbox-assets.sh check <repository> <out>
```

The guest base is reproducible: every byte is a function of `guest/build.py`'s Debian pins, `guest/init`, `guest/udhcpc.sh` and the static agent, which is built with `--locked` and its paths remapped. `check` rebuilds both guest bases into `<out>/work/guest/` and prints one line per asset, either `<file>: the lock's (<sha256>, <size> bytes)` or `<file>: DIFFERS: the lock has ..., <path> is ...`, and fails if any differs.

The root filesystem is not reproducible across days, because `sandbox/Dockerfile` does not pin what apt and pip install. So `check` does not rebuild it; for each root filesystem file given in `<out>/release/` it checks that the file has the lock's sha256 and size, and says `not given, not checked` for one that is absent.

Any change to the guest agent's source (the `mcbench-sandbox` crate and the crates it links, `native/Cargo.lock`), to the guest recipe (`guest/build.sh`, `guest/build.py`, `guest/init`, `guest/udhcpc.sh`) or to the toolchain changes the guest base's hash, and `check` shows it as `DIFFERS` for both architectures. The lock is then regenerated with `build --only guest-base` and published as a new release. Any change to `sandbox/Dockerfile` or to the root filesystem recipe (`rootfs/build.sh`, `rootfs/normalize.py`, its pinned base images or pasta) needs `build --only rootfs`, which `check` cannot detect; the root filesystem's `image.json` records the Dockerfile's sha256 it was built from.

## The package lists

`build` writes, under `<out>/lists/`, what each asset it built holds, read from the asset itself. `sources.sh` collects each package's source from these lists.

| File | From | Columns |
|---|---|---|
| `guest-base-<arch>.packages.tsv` | the asset's `manifest.json` (the Debian snapshot instant in its first comment line) | package, version, architecture, `.deb` file, sha256 |
| `rootfs-<arch>.dpkg-status` | the asset's `rootfs/var/lib/dpkg/status`, as it is | dpkg's own format |
| `rootfs-<arch>.pip.tsv` | every `*.dist-info/METADATA` under a `site-packages` folder in the asset | name, version, folder |
