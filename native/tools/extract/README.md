# The release-time extractor

The native app ships the data derived by running Minecraft's code as one zstd bundle embedded in the binary (docs/native-app.md, Minecraft data). This folder makes it, on the release machine only. Nothing it writes is committed, pushed or published by a script: the bundle is a file of the release machine, and publishing it as a release asset is a release step the owner approves each time.

| File | What it does |
|---|---|
| `cache.js` | Fills the cache with the repository's own pins and code: Paper's jar laid out by its launcher (`server/paper-files.js`), the BlueMap jar and Mojang's client mappings (`server/bluemap.js` `ensureDownloads`), Mojang's version file and client jar by the sha1s of Mojang's version manifest |
| `extract.sh`, `McbenchExtract.java`, `prep.js` | One JVM runs every program today's first start runs, unchanged (`McbenchEngine`'s dump, the loot tables, `DataFixReference --dump`, `McbenchBlockTable`, and `McbenchBake` with `McbenchClient`, `McbenchBlockEntities`, `McbenchEntities` and `McbenchEntityAssets` in BlueMap's jar), compiled as today's runs compile them, in a work folder laid out as today's (`prep.js` uses `server/bluemap.js` `configFiles` and `server/minecraft-dimension.js`); then Minecraft's night (`server/McbenchNight.java`, below) |
| `bundle.sh` | Stages the shipped files by their bundle names, makes the client-jar manifest (`mcbench-data stage`), the archive (`mcbench-data tar`), compresses it with `zstd -19 -T1 --check`, and writes the lock file (`mcbench-data lock`) |
| `regenerate.sh` | The regenerate-and-compare test: runs `extract.sh` and `bundle.sh` from the cache and requires every file and the bundle to be the tracked `native/minecraft-data.lock`'s, byte for byte |
| `costab.sh`, `McbenchCosTab.java` | The COS_TAB check: Minecraft's own `Mth.atan2` with the shipped `COS_TAB` and with an arm64 JVM's (FdLibm's `StrictMath.cos` of Mth's `ASIN_TAB`) over a fixed sample of 3,947,203 minecart movements; fails if any float yaw differs |
| `compare-today.sh` | Compares an extractor work folder with what today's separate steps (the Node app's first start) made, file by file |

`mcbench-data` is a binary of `mcbench-setup` (`native/crates/setup/src/bin/mcbench-data/`); the scripts run it with `cargo run --release` from `native/`.

## The cache

```
<cache>/paper/                      Paper 1.21.10 build 130 laid out (paper-1.21.10-130.jar, versions/, libraries/)
<cache>/bluemap/bluemap-5.22-cli.jar
<cache>/bluemap/minecraft-client-1.21.10-mappings.txt
<cache>/1.21.10.json                Mojang's version file
<cache>/client.jar                  Mojang's client jar
```

`node native/tools/extract/cache.js <repository> <cache> <jdk home>` fills it (about 160 MB; a file it holds is not downloaded again). The JDK is Temurin 25.0.5+8, the one `server/tools.js` pins (today's first start installs it in the tools folder). Node runs `cache.js` and `prep.js` only, at release time.

## A release

```
node native/tools/extract/cache.js . <cache> <jdk>
native/tools/extract/extract.sh . <cache> <jdk> <work>
native/tools/extract/bundle.sh <cache> <work> <out> [<release asset address>]
native/tools/extract/costab.sh <cache> <jdk> <out>/files/blocks.txt
```

`<out>/minecraft-data.lock` replaces `native/minecraft-data.lock` when the bundle is released (with the release asset's address in its `url` line), and `MCBENCH_DATA_BUNDLE=<out>/minecraft-data-1.21.10.tar.zst` builds `mcbench-setup` with `--features shipped-data`, as every release build does. Without the feature the crate builds without the bundle (`native/check.sh`); with it and without `MCBENCH_DATA_BUNDLE`, the build fails saying how to make or fetch the bundle; with a bundle that is not the lock's, it fails naming both sha256s.

## The bundle

A POSIX ustar archive of the thirteen files below, in this order, every header's mode 0644, owner 0 and time 0, compressed by the `zstd` program at level 19 on one thread with its frame checksum. The lock file (`native/minecraft-data.lock`) is the bundle's manifest: the Minecraft version, the bundle's sha256 and size, the address the build fetches it from, the `zstd` version that compressed it, and each file's name, sha256 and size. The app decompresses it with `ruzstd` (pure Rust, which checks the frame checksum), checks its sha256 against the lock, and unpacks it once into the data folder (`minecraft/1.21.10-<first 16 digits of its sha256>/`).

| Bundle name | From the work folder | Today's file |
|---|---|---|
| `blocks.txt` | `out/blocks.txt` | `core/minecraft/1.21.10/blocks.txt` |
| `loot.json` | `out/loot.json` | `core/minecraft/1.21.10/loot.json` |
| `datafix.txt` | `out/datafix.txt` | `core/minecraft/1.21.10/datafix.txt` |
| `block-table.tsv` | `out/block-table-paper-1.21.10-130-<format>.tsv` | `data/minecraft-live/paper/block-table-…tsv` |
| `render-layers.tsv`, `tints.tsv` | the same names | the bake folder's |
| `mcbench-pack.bin`, `mcbench-entities.json`, `settings.json` | `web/` | the bake folder's `web/` |
| `map-settings.json` | `web/maps/live/settings.json` | the bake folder's |
| `blocks.json` | `out/generated/reports/blocks.json` (Minecraft's data generator) written by `mcbench-data stage` as misode/mcmeta writes its summary | misode/mcmeta's `blocks.json`, downloaded at first start (`server/assets.js`): byte for byte the same |
| `client-jar.json` | made by `mcbench-data stage` | none: misode/mcmeta's files and the whole client jar today |
| `night.json` | `out/night.json` (`McbenchNight`) | none: the page draws no night of Minecraft's |

`blocks.txt` keeps its `mthtab COS_TAB` line. The plan drops it, but the engine reads `Mth.COS_TAB` from it (`core/src/registry.rs`, `mthtab`) for `Mth.atan2` (`core/src/mth.rs`) and has no other source of it; `core/` does not change in stage three. The bundle made on adipc (x86_64) carries x86_64's values; the line's last bits differ on arm64 (the spike's finding), so on the Mac the native app's table differs from the one today's app extracts there in those bits.

Regenerating the bundle includes `costab.sh` (after `bundle.sh`, or beside `regenerate.sh` on its `bundle/files/blocks.txt`). The engine reads the table only in `Mth.atan2`, for a minecart's yaw, which is a float. The check runs Minecraft's own `Mth.atan2` in the JVM with both tables over SplittableRandom(20261117)'s 4,000,000 draws of four kinds (dx, dz uniform in [-0.5, 0.5); in [-8, 8); diagonals; thousandths in [-0.4, 0.4]), keeping those with dx^2 + dz^2 > 0.001 as AbstractMinecart does. It fails if any float yaw differs. For 1.21.10: 10 of 257 entries differ, 474,344 of 3,947,203 atan2 doubles differ, 0 float yaws differ. It is a JVM check only: the engine's `atan2` is private to `core/`, and a Rust copy of it in a test, with an FdLibm cos of its own, would check a third implementation rather than Minecraft's or the engine's.

`blocks.json` is the block registry build programs are checked against. Its text reaches models (`server/mc-dsl.js`: an unknown block's closest matches, a state's valid names and values, in its order), so it must be today's file byte for byte. `McbenchExtract` runs Minecraft's data generator (`net.minecraft.data.Main --reports`) after everything else, and `mcbench-data stage` writes its blocks report by misode/mcmeta's rule (`main.py` at commit 3d63d61bcb7421a0d8ddda656bc851cc178067dd: each block without `minecraft:`, `[properties, default state's properties]` in the report's order, blocks sorted, `json.dump` with `(',', ':')` separators and a newline). The result has the sha256 `server/assets.js` pins for mcmeta's file (178a1209...). It is not made from the engine's table, which orders properties by name, where the report lists `type` first in 12 blocks.

The client-jar manifest names the jar (its address, size and sha1 from Mojang's version file) and every entry the app fetches, with its local header's offset, where its stored bytes begin, its stored and full size, its compression, its CRC32 and the sha256 of its bytes: each material's texture (BlueMap's `textures.json` `resourcePath`s, in order), each texture of the entity capture's atlas, each block's particle texture (the icon's; found over the jar's block states and models by `server/block-icon.js`'s rule), the animation files of those, Minecraft's atlas definitions, and the armour trims' palettes and base textures. For 1.21.10: 2,190 entries, 740,213 bytes stored.

What is never in the bundle: textures, `textures.json`, the entity atlas PNG, `.mcmeta` files, sounds, jars, mappings, `en_us.json`, source, BlueMap's web app.

## Minecraft's night

`McbenchExtract` runs `server/McbenchNight.java` last, once every other file is written, in a client loader of its own (it stands in for LWJGL's memory functions, which the bake's client has not), and writes `out/night.json`: what Minecraft 1.21.10's client computes at the level's time 6000 and 18000 under its default options (the lightmap's uniforms and its 16 x 16 texels by `core/lightmap.fsh`, the sky and fog colours of every overworld biome, the sky pass's draws with the star and moon geometry, the moon's phase and texture path), each value's source class and method named in the file. The live view draws night from it (`ui-seam-viewer-renderer-answers.md`, Day and night). It holds no texture: the moon and the sun are named by their path in the client jar. It is made in 6 to 7 s of the run, and the other files of a run are the same bytes with it and without it (57 files compared on 2026-11-17).

`regenerate.sh` holds it to the lock byte for byte, as every other file.

## The rollback build

The extractor stays runnable from a path: `extract.sh` takes the repository, the cache and the JDK as arguments and writes only into its work folder, and `mcbench-data stage` turns a work folder into the files `mcbench_setup::Data::at` reads. A rollback build that extracts on the user's machine would run these with a bundled trimmed Java runtime (the firststart spike's `jlink/`) and use the staged folder in place of the embedded bundle.
