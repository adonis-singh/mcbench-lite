//! # mcbench-setup
//!
//! First start and everything the app needs on disk before a turn: the
//! data folder per platform, the one-instance lock, the shipped Minecraft
//! data (made at release time by the extractor, `native/tools/extract/`,
//! embedded as one zstd bundle pinned by sha256 in
//! `native/minecraft-data.lock`), the texture fetch from inside Mojang's
//! client jar by HTTP range requests and the bake in Rust, and the pinned
//! downloads (the `bash` root filesystem, the provider CLIs). It reports each
//! step's progress for the setup screen. It replaces `server/setup.js`,
//! `tools.js` (the download pins), `assets.js`, `block-icon.js`,
//! `block-tints.js`, `minecraft-facts.js` (with `mcbench_world::facts`),
//! `minecraft-tables.js` (on users' machines: the tables are shipped),
//! `pid-lock.js` and `progress.js`.
//!
//! ## Owner
//!
//! The stage-three session. [`Step`] is a seam with the stage-four session
//! (the setup screen shows it).
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | the core | the data folder ([`data_folder`]), [`InstanceLock`], [`Ready`]: the shipped data's folder for `mcbench_engine::Engine::start`, `mcbench_render::Assets` for the renderer | the binary calls [`start()`] once, before the window has anything to send, and again for a retry | setup tasks on the async runtime; the unpack and the bake on `spawn_blocking` |
//! | interface | [`Step`]s as they change | [`Steps`] calls its callback with the whole list; the core forwards it to `mcbench-ui` | setup tasks to the interface thread |
//! | sandbox | the root filesystem's and (macOS, Windows) the guest base's file and sha256 ([`shell::files`], [`shell::ShellFiles`]) | the core's shell step ([`Shell`], run by [`start()`]) asks [`shell::files`] for them, makes the platform's sandbox of them and prepares it | a setup task |
//! | providers | the CLIs' pins ([`pins::programs`]) and [`pins::install`] | `mcbench-providers` (the port of `cli-install.js`) calls them on request | its task |
//! | network | range requests to `piston-data.mojang.com` for the textures; the CLIs' pinned downloads; the model's shell's pinned assets (a shipped build) | this crate fetches, checks every byte against its pin and keeps nothing that fails | setup tasks |
//!
//! ## Behaviour defined by
//!
//! Today's tests (docs/native-app.md, Appendix A), ported where the native
//! app keeps the behaviour (the crate's tests name each): `setup.test.js`
//! 7, `setup-status.test.js` 5 (the rules of the view; the page's wording
//! is the interface's), `tools.test.js` 19 (the rules of pinned
//! downloads; the JDK, rustup, Chromium and the engine are not downloaded
//! any more), `assets.test.js` 4 (misode/mcmeta's files are not downloaded
//! any more: the registry is shipped, and a test holds it to the sha256
//! today's pins), `block-icon.test.js` 3, `block-tints.test.js` 2,
//! `block-tints-minecraft.test.js` 3, `minecraft-facts.test.js` 2 (in
//! `mcbench-world`), `pid-lock.test.js` 3; `cli-install.test.js` 5 is
//! `mcbench-providers`'. Beside them the differentials: the extractor's
//! files against today's steps byte for byte
//! (`native/tools/extract/compare-today.sh`), the Rust bake against
//! today's bake on all 2,049 materials (`mcbench-data compare`), and the
//! icons, tints, facts and registry against Node's for every block
//! (`tests/differential.rs`).
//!
//! ## Decided
//!
//! The native app's steps ([`steps::native_steps`]): `data` (the shipped
//! data unpacked), `textures` (fetched from Mojang and baked) and `shell`
//! (optional, in the background: the files [`shell::files`] names, given to
//! the platform's sandbox, which is made of them and prepared; a shipped
//! build fetches the release's pinned assets, `native/sandbox-assets.lock`,
//! from the lock's `url` or the build's `MCBENCH_SANDBOX_ASSETS_URL` into
//! `<data folder>/sandbox-assets/`, checked by size and sha256, and reads no
//! variable for them; any other build takes the files
//! `MCBENCH_SANDBOX_ROOTFS`, `MCBENCH_SANDBOX_GUEST` and their `_SHA256`
//! name and fetches nothing).
//! Today's seven other steps are
//! gone with Java, Paper, BlueMap, the engine downloads and Chromium. On
//! Windows the WSL check is the shell step's first work, and the step is
//! unavailable without WSL, as today's is without Docker. The provider CLIs
//! are not a first-start step: as today, they are installed when the person
//! asks for one ([`pins::install`]).
//!
//! The data folder's lock ([`InstanceLock`]) is today's server's file,
//! `server.pid`, taken by `pid-lock.js`'s steps, so the native app and
//! today's localhost app on one folder (`MCBENCH_DATA_DIR`) refuse each
//! other instead of both writing its database.
//!
//! The shipped data is unpacked to disk (`minecraft/<version>-<sha256>/`,
//! about 24 MB written once) rather than served from memory, because the
//! engine and the renderer take paths (`Engine::start(tables)`,
//! `Assets`); a later start finds the folder and does nothing.
//!
//! [`StepState`] carries today's states (a download's bytes, the seconds a
//! done step took, `unavailable`), and [`start()`] is async with a
//! cancellation token.
//!
//! The block registry build programs are checked against is made by the
//! extractor from Minecraft's data generator's report, byte for byte
//! today's `blocks.json`, and shipped in the bundle ([`registry`] says why it is not
//! made from the engine's table). see_build's block atlas is the bake's own
//! layout (`atlas.png`, `atlas_map.json`, keyed as mcmeta's: `bake::own_layout`),
//! made from the fetched textures; no file of misode/mcmeta's is shipped.
//!
//! ## Open
//!
//! The rollback build (one cargo feature: a trimmed Java runtime and the
//! extractor run on the user's machine) is a stage-four deliverable. It
//! needs from this crate one more source of [`Data`]: the extractor's
//! output folder (`native/tools/extract/bundle.sh`'s `files/`), which
//! [`Data::at`] already reads, in place of `Data::shipped` (feature `shipped-data`).

pub mod bake;
pub mod bundle;
pub mod data;
pub mod fetch;
pub mod folder;
pub mod icon;
pub mod instance;
pub mod jar;
pub mod pins;
pub mod registry;
pub mod shell;
pub mod start;
pub mod steps;
pub mod tints;
pub mod wsl;

pub use data::Data;
pub use folder::{data_folder, logs_folder, tools_folder};
pub use instance::InstanceLock;
pub use start::{Ready, Shell, Textures, start};
pub use steps::{Bytes, Needs, Step, StepError, StepState, Steps};
