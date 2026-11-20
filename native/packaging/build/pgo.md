# The engine's profile in a release build

The released engine program (`mcbench-core`, built by `scripts/build-engine.js` and `scripts/release-engine.js`) is optimised with a profile from `core/pgo/<target>.profdata`. That profile does not apply to the workspace's build: cargo gives `mcbench_core` another crate hash in `native/`, so none of its function names match (`native/Cargo.toml`, the comment above `[profile.release]`). Without a profile the in-process engine was measured 8 to 20% slower than the released program. For Apple silicon macOS, x86_64 Linux and x64 Windows the release build uses the engine's pinned profile, recorded on the owner's machines with the corpus (section The engine's pinned profile, the owner's decision of 2026-11-18). Intel macOS and ARM Linux have no pinned profile, and their release build uses no profile (the ruling of 2026-11-20). A profile changes how the compiler lays out and inlines code; it changes nothing the engine computes.

## The three cases

`build/linux.sh`, `build/macos.sh` and `build/windows.ps1` decide the same way for `mcbench-lite`. The build child (`native/build-child`, V8) and the sandbox agent are always built without a profile.

| Case | What the script does |
|---|---|
| `MCBENCH_PGO_PIN=<recording>` | records the pinned profile: the steps below, then `<out>.profdata` and `<out>.record` (macOS `<out>/mcbench-lite.profdata` and `.record`) are the files to commit under `native/packaging/build/pgo/` |
| `native/packaging/build/pgo/<target>.profdata` exists | the release build with `-Cprofile-use=<that file>` and `-Cllvm-args=-pgo-warn-missing-function`, no instrumented build and no replay; the engine crate's lines of the log are judged against the `.record` (section The engine's pinned profile) |
| neither | the release build with no profile: `cargo build --release --locked --features shipped-data -p mcbench-lite --target <target>` alone, and no profile or profile log is written. Today `aarch64-unknown-linux-gnu` and `x86_64-apple-darwin` |

`windows.ps1 -NoProfile` gives the third case's build on Windows whatever the tree holds (section Windows builds without a profile).

## Recording a pinned profile: the steps

| Step | What runs |
|---|---|
| 1. Instrumented build | `cargo build --release --locked --features shipped-data -p mcbench-lite --target <target>` with `-Cprofile-generate=<raw folder>`, into a target folder of its own (Linux `/cache/target-pgo`, macOS `$CARGO_TARGET_DIR-pgo`, Windows `<target folder>-pgo`) |
| 2. Workload | `node native/packaging/build/pgo-replay.js <corpus recording> <data bundle> <work> <instrumented mcbench-lite>`, with Node.js 26.13.1 from `node-pins.txt` (downloaded, sha256 checked). It loads nothing of `server/` and writes one `.profraw` per program into the raw folder |
| 3. Merge | `llvm-profdata merge -o <profile> <raw folder>/*.profraw`, the `llvm-profdata` of the toolchain's own `llvm-tools` component, then the reduction to the engine crate (section The engine's pinned profile) |
| 4. Final build | the same cargo command with `-Cprofile-use=<reduced profile>` and `-Cllvm-args=-pgo-warn-missing-function`, its output kept as the build's log; the engine crate's lines without a record are counted into `<out>.record`, and the profile's size and sha256 are printed |

Both builds are given `--target`, so the flags reach the shipped program's crates and not the build scripts or procedural macros (cargo applies `build.rustflags` to those too when no `--target` is given): build scripts would otherwise write profiles of their own during the build and draw warnings in step 4. The profile applies because cargo gives every crate the same hash with and without the two flags, as long as everything else about the two builds is the same.

On Linux and Windows the flags are given as `--config build.rustflags=[...]`, the form `native/Cargo.toml` names. On macOS they are appended to `CARGO_ENCODED_RUSTFLAGS`, which `macos.sh` already sets for its path remapping and which would make cargo ignore `build.rustflags`. The instrumented Linux build also gets `-Clink-arg=-Wl,--defsym=xkb_utf32_to_keysym=0`: an instrumented program keeps every function, among them the `xkbcommon` crate's wrapper of `xkb_utf32_to_keysym`, which Debian 10's libxkbcommon 0.8 does not export, and which the shipped build drops as unused. The instrumented program alone defines that symbol as address 0 so that it links and loads; the workload never reaches the window toolkit.

Where each tool comes from:

| Need | Debian 10 build container (Linux) | macOS | Windows |
|---|---|---|---|
| Rust 1.99.1 | the pinned `rustup-init` in the container, toolchain from `native/rust-toolchain.toml` | rustup, the same file | rustup, the same file |
| `llvm-profdata` | `rustup component add llvm-tools`; `$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-profdata` (or `aarch64-...`) | the same component; `.../lib/rustlib/<target>/bin/llvm-profdata` | the same component; `...\lib\rustlib\x86_64-pc-windows-msvc\bin\llvm-profdata.exe` |
| Node.js | `node-pins.txt`'s `linux-<arch>` tarball, fetched on the host and unpacked in the container (Node 26 runs on glibc 2.28) | `macos-<arch>` tarball | `windows-x86_64` zip |
| Minecraft's tables | the data bundle (`MCBENCH_DATA_BUNDLE`), which every release build already takes | the same | the same |

Node.js is needed only when a profile is recorded; a release build with the pinned profile or with none downloads nothing for it.

The instrumented program runs on the machine that records, so a target's profile is recorded on a machine of its architecture. `macos.sh` refuses a target that is not the Mac's own architecture: an x86_64 build made on an Apple silicon runner could run its instrumented program only under Rosetta, so the release workflow builds `x86_64-apple-darwin` on the `macos-15-intel` runner and `aarch64-apple-darwin` on `macos-15`. Linux builds each architecture on a machine of that architecture.

## The corpus recording

`scripts/core-pgo.js` records `core/pgo/` with runs of `scripts/core-bench.js` on the instrumented released program, the first of them the corpus of community builds (`MCBENCH_CORPUS`). A pinned profile is recorded from the same corpus run, as a recording: `MCBENCH_CORPUS=<corpus> node native/packaging/build/pgo-workload.js <repository> <data bundle> <work> <corpus recording>` writes the requests once, on macOS or Linux, from the repository's `server/` and `scripts/` and the bundle's tables, without an engine: the `Engine` of `server/engine.js` it starts runs the script itself as its program, which keeps every request line and input file and answers each request as done, so no request depends on an answer. It holds one engine process's requests, `01-worlds`: `core-bench.js`'s corpus sequence on each build six times in a row (a fresh placement, every lever toggled and button pressed, 1000 ticks with light in requests of 100, the levers toggled again at tick 500, then a seeded `VERIFY` of `core-bench.js`'s experiment). The corpus is the benchmark's, so the recording is kept in a scratch folder of the machine that records, never under a path the public export copies.

`pgo-replay.js` loads nothing of `server/` and no npm package, so it runs on macOS, Linux and Windows. It unpacks the recording and the bundle's three engine tables, and for each folder in order starts `<program> headless engine <tables>` in a folder of its own with the recording's `world/` (the data pack `Engine` writes beside an engine), sends each line with `@<n>` replaced by the input file and `@out` by an output of its own, one request after the answer to the one before, and closes the engine's input at the end, so the instrumented program returns from `main` and writes its profile; an engine that exits otherwise than with code 0 fails the build.

Beside the recording is `<recording>.counts` (for `.tar.zst`), one line per folder: its name, its number of requests and the number the engine refused, from a replay on Linux. Every replay compares each folder's counts with it and fails when they differ, so a profile of an engine that answered otherwise is never used; `pgo-replay.js ... --write-counts` on Linux writes the file from a replay after the recording is made.

`pgo-replay.js --times` also prints, per folder, the seconds summed by request word (`PLACE`, `TICK`, `VERIFY`, `INPUT`, the others), each from the request's write to its answer, which makes a replay a measurement on a machine where `core-bench.js` does not run (Windows).

`pgo-workload.js` does not run `core-bench.js`, which lays out the Paper server before any run (a download, and Paper's launcher on a JDK) although the Rust engine reads nothing of it; it repeats `core-bench.js`'s `experiment()` and the corpus sequence.

## Telling that the profile was applied

From the build log of a build with the pinned profile (Linux `<out>.pgo.log`, macOS `<out>/pgo.log`, Windows `<Out>.pgo.log`): only the engine crate's code units (`mcbench_core.<hash>-cgu.<n>`) are judged, as below, and the script prints `pgo: the engine's profile applied: <n> engine functions with a hash mismatch, <n> without a record`. On macOS ld64's `-dead_strip` removes unused functions from the instrumented program together with their records, so the recording's own final build names those as without profile data; their number is the `.record`'s. A profile recorded on another build (another crate hash, another target, another `--target` choice) has no record of the engine's functions, so the log names them, and the build fails. A missing profile file fails at once in rustc (`-Cprofile-use` names a file that does not exist). The script also prints each step's seconds.

A build with no profile prints `pgo: no pinned profile for <target>, so mcbench-lite is built with none: <n> s` (Windows: `build without a profile: <n> s`), runs no instrumented build and no replay, passes no `-Cprofile-use`, and writes no profile and no profile log.

From the binary alone it cannot be told: a profile leaves no marker, only a different layout. Two binaries of one commit built with and without the profile differ in their bytes; an instrumented one has the `__llvm_prf_cnts` and `__llvm_prf_data` sections (`objdump -h`), which a shipped one must not have.

Where the profile is kept: Linux `<out>.profdata` beside the payload folder, macOS `<out>/mcbench-lite.profdata` beside the bundle and the archive, Windows `<Out>.profdata` beside the payload folder. It is a build output, not shipped.

## The engine's pinned profile

The three pinned files hold the engine crate's records alone, recorded on the corpus, and are given to the final build as `-Cprofile-use=<file>` with no instrumented build and no replay. They are recorded again when the engine changes (its sources, its version, its dependencies, the toolchain), not at every app release.

| Target | File | Recorded on | Command |
|---|---|---|---|
| `x86_64-unknown-linux-gnu` | `native/packaging/build/pgo/x86_64-unknown-linux-gnu.profdata` and `.record` | the Linux build machine, in `linux.sh`'s container | `MCBENCH_PGO_PIN=<corpus recording> MCBENCH_DATA_BUNDLE=<bundle> native/packaging/build/linux.sh <repository> x86_64 <out>`, then copy `<out>.profdata` and `<out>.record` |
| `aarch64-apple-darwin` | `native/packaging/build/pgo/aarch64-apple-darwin.profdata` and `.record` | the owner's Apple silicon Mac | `MCBENCH_PGO_PIN=<corpus recording> MCBENCH_DATA_BUNDLE=<bundle> MCBENCH_UPDATE_KEY_FILE=<key> MCBENCH_UPDATE_FEED=<feed> CARGO_TARGET_DIR=<target> native/packaging/build/macos.sh <repository> aarch64-apple-darwin <out>`, then copy `<out>/mcbench-lite.profdata` and `<out>/mcbench-lite.record` |
| `x86_64-pc-windows-msvc` | `native/packaging/build/pgo/x86_64-pc-windows-msvc.profdata` and `.record` | the Windows VM | `$env:MCBENCH_PGO_PIN='<corpus recording>'` with the build's other variables, `.\native\packaging\build\windows.ps1 -Repository <repository> -LinuxFiles <folder> -Out <out>`, then copy `<out>.profdata` and `<out>.record` |
| `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu` | none | not recorded | the release build has no profile (section The three cases) |

`core/pgo/` stays as it is: the localhost app's released engine program is still built with it.

Recording the engine's profile, the same steps on all three: the instrumented build as above; `pgo-replay.js` sending the corpus recording to it; the merge; the reduction to the engine crate; the final build with the reduced profile, whose log gives the number of engine functions without a record; `<out>.profdata` and `<out>.record` (one line, `engine-missing<TAB><n>`) written and the profile's size and sha256 printed. The corpus recording is `MCBENCH_CORPUS=<corpus> node native/packaging/build/pgo-workload.js <repository> <bundle> <work> <corpus recording>` (section The corpus recording), followed by `node native/packaging/build/pgo-replay.js <corpus recording> <bundle> <work> <program> --write-counts` on Linux for its `.counts`. It holds the benchmark's builds, so it is kept in a scratch folder of the machine that records, never under a path the public export copies, and only the reduced profile is committed (checked below).

Why the engine crate keeps its records across app releases. The workspace's crates take the workspace's version into their crate hash, so a version bump renames all their functions; the engine crate (`core/`, its own package and version, a path dependency) keeps its hash while its version, its dependencies, its features and the toolchain stay. Shown on Linux on 2026-11-18: the tree at 0.1.0 built with its own profile, then `native/packaging/set-version.sh 0.1.1` and the same build with that profile. Cargo found `mcbench-core` fresh (not rebuilt: its metadata and so its crate hash unchanged) and rebuilt the 16 workspace crates; the build log named 23,323 functions without a record, every one in a workspace crate's code units (10,079 in `mcbench_ui`, 3,087 in `mcbench_lite`, down to 79 in `mcbench_render`; 98 of them engine generics instantiated in a workspace crate), none in the engine crate's, none in a crates.io crate's (all fresh) and none of std's (prebuilt), and no hash mismatch.

The reduction. The toolchain's `llvm-profdata merge` keeps or drops records by a regular expression on their names (`--function`, `--no-function`). The pinned file is the merge of the records of the engine crate's own code units (`--function='^mcbench_core\.'`, the local functions, 4,254 of the corpus profile's) and of the global functions whose names hold the crate (`--function=mcbench_core --no-function='-cgu\.[0-9]+;'`, 661: the engine's own and std's generics instantiated with engine types). The engine's generics instantiated in a workspace crate's units (96) are left out: their names carry that crate's hash, which the next version bump changes. On x86_64 Linux the reduced file is 1.9 MB (1,910,136 bytes, 4,915 functions) of the whole profile's 52 MB (136,571 functions), and holds 99.996% of its counts.

The rule with a pinned profile. Only lines of the engine crate's code units (`warning: mcbench_core.<hash>-cgu.<n>: ...`) are judged; the workspace's functions have no record and are not reported. The build fails when one of them is a hash mismatch (an engine function changed) or when more of them are without a record than the `.record` file's `engine-missing` (an engine function was added, or the engine's crate hash changed and every one lost its record); the message says to record the profile again. On Linux and Windows `engine-missing` is 0. On macOS the recording's instrumented link strips dead engine functions with their records, so its recording build counts them and the release builds may have no more: the same rule, with the number the Mac measured.

Measured on the Linux build machine, 2026-11-18 04:11 to 04:26 (one-minute load 15, 17, 20, 26, 17 and 7 at the runs' starts and ends), `core-bench.js` on the 82 corpus builds as in Measured below, 5 runs of 5 timed rounds, medians and median ratios. All builds are of one commit and differ only in the profile; the four profiles were taken from one instrumented build. The repository's recording is the one described at the start of Measured.

| Profile of the final build | place ms | tick ms | verify ms | place, tick, verify against the released program |
|---|---|---|---|---|
| the released program `mcbench-core` | 522 | 1168 | 2662 | 1, 1, 1 |
| none | 587 | 1362 | 2789 | 1.117, 1.166, 1.049 |
| the build's own, from the repository's recording | 535 | 1265 | 2628 | 1.027, 1.078, 0.990 |
| the engine's records from the corpus alone (the pinned form) | 517 | 1188 | 2566 | 0.995, 1.013, 0.964 |
| the same, merged with the build's own records of the other crates | 526 | 1189 | 2586 | 1.007, 1.020, 0.966 |
| the engine's records from the corpus and the repository's recording | 524 | 1186 | 2565 | 1.007, 1.014, 0.964 |

The committed x86_64 Linux file, `pgo/x86_64-unknown-linux-gnu.profdata` (1,910,136 bytes, 4,915 records, sha256 `58c5efcb3450172bdf81fb139ac232ad2c62e355e450f322e01d0a9d8339dd87`, `engine-missing 0`), was recorded through `linux.sh` with `MCBENCH_PGO_PIN` on 2026-11-18: the instrumented build (cached; 824 s cold), the corpus replay 7 s, the reductions, and the final build 546 s, which reported no engine function with a hash mismatch or without a record. A `linux.sh` run with the file in the tree then built the payload in 626 s with no instrumented build and reported the same. The build with it, measured 2026-11-18 04:46 to 04:57 (load 14 to 32) against the released program and the build without a profile as above: 0.986, 1.020, 0.968 (no profile 1.138, 1.159, 1.055); outputs agreed on the 82 builds in every round, and `core-parity.js` against the build without a profile gave 267, 0, 33 and 275, 0, 25 on the two unseen seeds. Before it was committed, `llvm-profdata show --all-functions` gave 4,915 records whose names are all `mcbench_core.<hash>-cgu.<n>;_R...` or `_R...` symbols, each with a hash, counters and counts and nothing else; none of the 82 corpus build names, no home path, machine name or file path appears in the dump or in the file's bytes (the names are stored uncompressed), and `export-public.js` refuses only the two vendored lines it refused before.

The engine's records alone are within 1.3% of the released program on every column and need no instrumented build and no replay in a release; the merge with the build's own records for the rest gives nothing measurable, so the pinned form is the engine's records alone, recorded on the corpus alone. (The measured file was reduced with `--function=mcbench_core` alone, 5,011 records; the committed form leaves out the 96 records of engine generics in workspace units, which a release build never matches.) The outputs agreed across all six engines in every round, and `core-parity.js --base <no profile> --rust <engine records alone>` gave 267 matched, 0 differed, 33 refused by both (300 cases, seed 500000) and 275, 0, 25 (seed 600000, `--drops`).

## Measured

These measurements were taken while a target without a pinned profile recorded its own profile at every release, from a recording the repository kept, `native/packaging/build/pgo-recording.tar.zst` with its `pgo-recording.counts` (in git history up to `native-app` 59deb88c; removed when those targets moved to no profile on 2026-11-20). The workloads below were made by the `pgo-workload.js` of that time from what the repository holds: the first was `core-pgo.js`'s three fuzzer runs (200 cases each of `scripts/core-parity.js`'s `fuzzCase`, seeds 1, 100000 and 200000); the second added `core-bench.js`'s corpus sequence on generated worlds (the redstone fixture of `test/fixtures/redstone-speed-world.js`, `useCases()`, 24 cases of `scripts/orb-cases.js` and the first placement of each fuzz case); the third added three large worlds (the fixture tiled 4 by 3, a stone volume of 210 lit rooms, 2688 hoppers in 672 lines) ticked 2000 times. The repository's recording was the third workload's requests.

On an x86_64 Linux machine with 32 threads, 2026-11-18 02:34 to 02:53, with `scripts/core-bench.js` on 82 community builds of a corpus outside the repository (`MCBENCH_CORPUS`): each build placed fresh, ticked 1000 times with its levers and buttons used, and sent one seeded `VERIFY`, 5 timed rounds after 1 dropped, the engines in a rotating order each round; each figure is the sum over the 82 builds of each build's median, and the table gives the median of 5 such runs. The one-minute load average at the starts and ends of the runs was 21, 14, 15, 32, 8 and 16, from other sessions' work, so these are not speed-table timings on a quiet machine; the ratios matter more than the milliseconds, and a difference of 1 to 2% between two rows is within what the runs vary by.

| Engine | place ms | tick ms | verify ms | place, tick, verify against the released program |
|---|---|---|---|---|
| the released program `mcbench-core` of engine `ad327be5bdf0dd1a` (its pinned `linux-x64` asset, sha256 checked; `core/` is unchanged since) | 538 | 1208 | 2750 | 1, 1, 1 |
| `mcbench-lite headless engine` without a profile | 607 | 1398 | 2864 | 1.132, 1.152, 1.042 |
| with a profile of the three fuzzer runs alone (the first workload) | 536 | 1307 | 3304 | 0.993, 1.072, 1.201 |
| with a profile of the fuzzer runs and `core-pgo.js`'s corpus run on the same 82 builds, for comparison | 535 | 1220 | 2685 | 0.999, 1.000, 0.988 |
| with a profile of the second workload (the fuzzer runs and the corpus sequence on the small generated worlds) | 537 | 1298 | 2613 | 1.004, 1.057, 0.975 |
| with a profile of the third workload (the second with the three large worlds), recorded by the workload run live | 556 | 1291 | 2683 | 1.032, 1.058, 0.988 |
| with the profile `linux.sh` recorded by replaying `pgo-recording.tar.zst` (the third workload's requests) | 544 | 1307 | 2712 | 1.012, 1.071, 0.986 |

The ratios are the medians of the 5 runs' ratios. All six builds of `mcbench-lite` are of one commit and differ only in the profile; the second to fifth profiles were recorded on the same instrumented build. The comparison row's profile was recorded on the builds it is measured on, as `core/pgo/`'s was, and is not what a release build records. In an earlier set of 5 runs (2026-11-18 02:03 to 02:19, load 9 to 23) the third workload gave 1.014, 1.066 and 0.994 and the second 1.009, 1.077 and 0.991.

Placement and `VERIFY` are within about 1% of the released program; ticks stay 6 to 7% slower with every workload the repository can make, against 1.000 for the profile that saw the corpus. The builds whose ticks lose most are the corpus's large ones (displays and memories of 16,000 to 195,000 blocks) and its hopper and collector machines; the large generated worlds (tiled redstone, lit rooms, idle hopper lines) did not close the gap, so the tick work they lack is of another kind than size: the corpus builds' own mixes of components, and items moving for the whole run.

The 82 builds' outputs agreed across all seven engines in every round (placement blueprints, tick patches and entities, and the seeded `VERIFY` reports, hashed by `core-bench.js`, which names any difference). `scripts/core-parity.js --base <without profile> --rust <the replayed profile>` on fuzzer worlds the profile was not recorded on: 300 cases from seed 500000 at 100 ticks, 267 matched, 0 differed, 33 refused by both; 300 cases from seed 600000 with `--drops`, 275 matched, 0 differed, 25 refused by both; the third workload's live profile gave the same, and the released program against it at seed 500000 the same 267, 0 and 33.

`linux.sh` for x86_64 when it recorded its own profile, in the container at its default `--cpus 2 --memory 8g`, from a cold target folder: the instrumented build 996 s, the workload 58 s, the merge a few seconds, the final build 587 s, the whole script 1726 s. With the instrumented build cached (no source change) and the replay: the workload 42 s and the final build 437 s. The same build without a profile took 566 s, so the profile adds one instrumented build (595 to 996 s at two CPUs) and under a minute per target. The container's memory reached 7.98 GiB of its 8 GiB (page cache included) during an instrumented build's link and was not ended. The profile is 51.2 MB; the shipped `mcbench-lite` is about 57.5 MB.

## Windows builds without a profile

`windows.ps1 -NoProfile` builds `mcbench-lite.exe` once with the release profile and no profile-guided optimisation, the build a target without a pinned profile gets, even when the pinned profile exists. It is a switch for tests and measurements (the with and without timing of `pgo-replay.js --times`), and a release never passes it: the release workflow's Windows job builds with the engine's pinned profile, `native/packaging/build/pgo/x86_64-pc-windows-msvc.profdata`, recorded in the Windows VM on 2026-11-18 (1,827,864 bytes; instrumented build 1,075 s, the corpus replay 17 s, the engine's records reduced, the build with them: 0 engine functions with a hash mismatch, 0 without a record).

Two things had stood in the way, both seen in the VM that day. The first workload loaded the Node server's modules, and `server/tools.js` knows no Windows platform, so it stopped before any engine started; the recording and `pgo-replay.js` replaced it. Then the instrumented program wrote no profile although the replay ran whole with Linux's counts: `headless engine` ended with `std::process::exit`, which on Windows is `ExitProcess` and skips the exit handler LLVM's profile runtime writes from; since `native-app` 185c5fc1 it ends through the C runtime's `exit`, and the instrumented program writes its file.

## Not measured

Intel macOS and ARM Linux, which build with no profile, have not been built on a local machine, and their engine's speed with no profile is not measured on them (on x86_64 Linux a build with no profile measured 1.132, 1.152 and 1.042 against the released program, in Measured above). The in-process engine (the live world inside the app) shares the profiled code with `headless engine` and is not timed here; neither are the settle pool's children, `test_build`, or the app's start. `-pgo-warn-missing-function` is proven to warn in rustc (a program compiled against this profile names its functions), and the final build's log had none, but whether cargo shows LLVM's warnings for crates.io dependencies, which it builds with `--cap-lints allow`, is not checked; `mcbench-core` and the workspace's crates are path dependencies, whose warnings cargo shows. The speed table's rows on a quiet machine are not taken.

## After 2.0.0

2.0.1: record ARM Linux's and Intel macOS's profiles on the runner with a thin-LTO instrumented build (counters are inserted per crate before LTO, so a fat-LTO final build can use them); the fat-LTO instrumented build needs more than 14 GiB on ARM
